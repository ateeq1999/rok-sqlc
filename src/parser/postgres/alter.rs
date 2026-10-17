//! `ALTER TABLE` and `ALTER COLUMN` on a known table, and `DROP TABLE`.

use std::collections::HashSet;

use sqlparser::ast::{AlterColumnOperation, AlterTableOperation, ColumnDef, ObjectName};

use super::checks::apply_range_check;
use super::constraints::{add_constraint, add_key_constraint};
use super::enums::{EnumRegistry, restrict_to_values};
use super::names::{
    find_table, ident_name, normalize_schema, object_leaf_name, object_schema_name,
};
use super::table::{build_column, field_taken, rust_type};
use super::types::{declared_length, is_serial_type, map_type_in};
use crate::parser::diagnostics::TableFailure;
use crate::parser::naming;
use crate::schema::{BaseType, Column, Shape, Table, UniqueKey};

/// Applies a `DROP TABLE` to one name. A known table is removed (`removed`
/// sees it first), and so is every foreign key another table holds to it, as
/// `CASCADE` would: the referencing column stays, with its raw type. A table
/// that failed earlier is forgotten, failure included. Unknown names are
/// ignored, like `IF EXISTS`.
pub(super) fn drop_table(
    tables: &mut Vec<Table>,
    failures: &mut Vec<TableFailure>,
    name: &ObjectName,
    removed: impl FnOnce(&Table),
) {
    let leaf = object_leaf_name(name);
    let schema = normalize_schema(object_schema_name(name));
    failures.retain(|failure| {
        failure.table != leaf || normalize_schema(failure.schema.clone()) != schema
    });
    let Some(index) = find_table(tables, name) else {
        return;
    };
    let table = tables.remove(index);
    removed(&table);
    for other in tables.iter_mut() {
        other.foreign_keys.retain(|key| {
            key.referenced_table != leaf
                || normalize_schema(key.referenced_schema.clone()) != schema
        });
    }
}

/// Applies an `ALTER TABLE`'s clauses to a known table, in order. An error
/// names the clause that could not be applied.
pub(super) fn apply_alter(
    table: &mut Table,
    operations: &[AlterTableOperation],
    enums: &EnumRegistry,
    taken: &HashSet<String>,
) -> std::result::Result<(), String> {
    for operation in operations {
        match operation {
            AlterTableOperation::AddColumn {
                if_not_exists,
                column_def,
                ..
            } => add_column(table, column_def, *if_not_exists, enums, taken)?,
            AlterTableOperation::DropColumn {
                column_names,
                if_exists,
                ..
            } => {
                for name in column_names {
                    drop_column(table, &ident_name(name), *if_exists)?;
                }
            }
            AlterTableOperation::RenameColumn {
                old_column_name,
                new_column_name,
            } => rename_column(
                table,
                &ident_name(old_column_name),
                &ident_name(new_column_name),
            )?,
            AlterTableOperation::AddConstraint { constraint, .. } => {
                add_constraint(table, constraint, taken)?;
            }
            AlterTableOperation::AlterColumn { column_name, op } => {
                alter_column(table, &ident_name(column_name), op, enums)?;
            }
            AlterTableOperation::RenameConstraint { old_name, new_name } => {
                // Only primary key and unique constraint names are tracked;
                // renaming another constraint changes nothing here.
                let old_name = ident_name(old_name);
                if let Some(constraint) = table
                    .key_constraints
                    .iter_mut()
                    .find(|constraint| constraint.name == old_name)
                {
                    constraint.name = ident_name(new_name);
                }
            }
            // Clauses that leave the columns and keys as they are. Triggers
            // are followed by `CREATE` and `DROP TRIGGER` only.
            AlterTableOperation::EnableRowLevelSecurity
            | AlterTableOperation::DisableRowLevelSecurity
            | AlterTableOperation::SetOptionsParens { .. }
            | AlterTableOperation::ReplicaIdentity { .. }
            | AlterTableOperation::OwnerTo { .. }
            | AlterTableOperation::ValidateConstraint { .. }
            | AlterTableOperation::EnableTrigger { .. }
            | AlterTableOperation::DisableTrigger { .. }
            | AlterTableOperation::EnableAlwaysTrigger { .. }
            | AlterTableOperation::EnableReplicaTrigger { .. }
            | AlterTableOperation::EnableRule { .. }
            | AlterTableOperation::DisableRule { .. }
            | AlterTableOperation::EnableAlwaysRule { .. }
            | AlterTableOperation::EnableReplicaRule { .. } => {}
            other => {
                return Err(format!("unsupported ALTER TABLE clause `{other}`"));
            }
        }
    }
    Ok(())
}

/// Applies an `ALTER COLUMN` change, leaving the column in its final shape.
/// Keys, unique keys, and foreign keys name their columns, so they follow a
/// type change.
///
/// PostgreSQL rejects `SET DEFAULT` and `DROP DEFAULT` on identity and
/// `GENERATED` columns, so a generated column they reach is a serial one:
/// replacing or dropping its `nextval` default leaves it no longer generated.
fn alter_column(
    table: &mut Table,
    name: &str,
    operation: &AlterColumnOperation,
    enums: &EnumRegistry,
) -> std::result::Result<(), String> {
    let index = column_index(table, name).ok_or_else(|| missing_column("ALTER COLUMN", name))?;
    let is_key = table.primary_key.iter().any(|key| key == name);
    let column = &mut table.columns[index];
    let column_fail = |reason: String| format!("ALTER COLUMN: column `{name}`: {reason}");
    match operation {
        AlterColumnOperation::SetDataType { data_type, .. } => {
            if is_serial_type(data_type) {
                return Err(column_fail(format!(
                    "type `{data_type}` exists only in CREATE TABLE and ADD COLUMN"
                )));
            }
            let (base_type, sqlx_type) = map_type_in(data_type, enums).map_err(column_fail)?;
            column.rust_type = rust_type(&base_type, column.nullable);
            // The value range was fitted to the old type (`> 0` on an integer
            // is `>= 1`), so it holds only while the Rust type is the same.
            // Dropping it loosens the rule; the database keeps the check.
            if column.sqlx_type != sqlx_type {
                column.value_range = None;
            }
            column.sqlx_type = sqlx_type;
            column.declared_length = declared_length(data_type);
            // A value list restricts text only; one on another type is no
            // enum. A length range, too, holds for text only.
            if column.base_type() != BaseType::Text {
                column.value_list = None;
                column.length_range = None;
            }
        }
        AlterColumnOperation::SetNotNull => set_nullable(column, false),
        AlterColumnOperation::DropNotNull => {
            if is_key {
                return Err(format!(
                    "ALTER COLUMN: column `{name}` is in the primary key, so it cannot drop NOT NULL"
                ));
            }
            set_nullable(column, true);
        }
        AlterColumnOperation::SetDefault { .. } => {
            column.has_default = true;
            column.is_generated = false;
        }
        AlterColumnOperation::DropDefault => {
            column.has_default = false;
            column.is_generated = false;
        }
        AlterColumnOperation::AddGenerated { .. } => {
            if column.nullable {
                return Err(format!(
                    "ALTER COLUMN: column `{name}` must be NOT NULL before it can become an identity column"
                ));
            }
            column.has_default = true;
            column.is_generated = true;
        }
    }
    Ok(())
}

/// Makes `column` nullable or required, wrapping or unwrapping its Rust type.
fn set_nullable(column: &mut Column, nullable: bool) {
    if column.nullable == nullable {
        return;
    }
    let base_type = match Shape::of(&column.rust_type) {
        Shape::Optional(inner) if column.nullable => inner.to_owned(),
        Shape::Optional(_) | Shape::List(_) | Shape::Value(_) => column.rust_type.clone(),
    };
    column.rust_type = rust_type(&base_type, nullable);
    column.nullable = nullable;
}

pub(super) fn column_index(table: &Table, name: &str) -> Option<usize> {
    table
        .columns
        .iter()
        .position(|column| column.sql_name == name)
}

fn missing_column(clause: &str, name: &str) -> String {
    format!("{clause}: column `{name}` does not exist")
}

fn add_column(
    table: &mut Table,
    column_def: &ColumnDef,
    if_not_exists: bool,
    enums: &EnumRegistry,
    taken: &HashSet<String>,
) -> std::result::Result<(), String> {
    let name = ident_name(&column_def.name);
    if column_index(table, &name).is_some() {
        if if_not_exists {
            return Ok(());
        }
        return Err(format!("ADD COLUMN: column `{name}` already exists"));
    }
    let built =
        build_column(column_def, enums).map_err(|reason| format!("ADD COLUMN: {reason}"))?;
    if table
        .columns
        .iter()
        .any(|column| column.rust_name == built.raw.rust_name)
    {
        return Err(format!("ADD COLUMN: {}", field_taken(&built.raw)));
    }
    if let Some(constraint) = &built.primary_key {
        if !table.primary_key.is_empty() {
            return Err(format!(
                "ADD COLUMN: column `{name}` adds a second primary key"
            ));
        }
        table.primary_key = vec![name.clone()];
        add_key_constraint(table, constraint.clone(), vec![name.clone()], true, taken);
    }
    if let Some(constraint) = &built.unique {
        table.unique_keys.push(UniqueKey::new(vec![name.clone()]));
        add_key_constraint(table, constraint.clone(), vec![name.clone()], false, taken);
    }
    table.foreign_keys.extend(built.foreign_key);
    table
        .columns
        .push(built.raw.into_column(built.primary_key.is_some()));
    for (column, labels) in built.value_lists {
        if let Some(index) = column_index(table, &column) {
            restrict_to_values(&mut table.columns[index], labels);
        }
    }
    for check in built.range_checks {
        if let Some(index) = column_index(table, &check.column) {
            apply_range_check(&mut table.columns[index], check);
        }
    }
    Ok(())
}

/// Drops a column. Like PostgreSQL, dropping a key column drops the primary
/// key, unique keys, and foreign keys it belongs to.
pub(super) fn drop_column(
    table: &mut Table,
    name: &str,
    if_exists: bool,
) -> std::result::Result<(), String> {
    let Some(index) = column_index(table, name) else {
        if if_exists {
            return Ok(());
        }
        return Err(missing_column("DROP COLUMN", name));
    };
    table.columns.remove(index);
    if table.primary_key.iter().any(|key| key == name) {
        table.primary_key.clear();
        for column in &mut table.columns {
            column.is_primary_key = false;
        }
    }
    table
        .unique_keys
        .retain(|key| !key.columns.iter().any(|column| column == name));
    table
        .key_constraints
        .retain(|key| !key.columns.iter().any(|column| column == name));
    table.foreign_keys.retain(|key| key.column != name);
    Ok(())
}

fn rename_column(table: &mut Table, old: &str, new: &str) -> std::result::Result<(), String> {
    let index = column_index(table, old).ok_or_else(|| missing_column("RENAME COLUMN", old))?;
    if column_index(table, new).is_some() {
        return Err(format!("RENAME COLUMN: column `{new}` already exists"));
    }
    let rust_name =
        naming::rust_identifier(new).map_err(|reason| format!("RENAME COLUMN: {reason}"))?;
    if let Some(other) = table
        .columns
        .iter()
        .find(|column| column.sql_name != old && column.rust_name == rust_name)
    {
        return Err(format!(
            "RENAME COLUMN: column `{new}` maps to Rust field `{rust_name}`, which is already taken by column `{}`",
            other.sql_name
        ));
    }
    let column = &mut table.columns[index];
    column.sql_name = new.to_owned();
    column.rust_name = rust_name;

    let rename = |column: &mut String| {
        if column == old {
            *column = new.to_owned();
        }
    };
    table.primary_key.iter_mut().for_each(rename);
    for key in &mut table.unique_keys {
        key.columns.iter_mut().for_each(rename);
    }
    // PostgreSQL keeps a constraint's name when a column is renamed.
    for key in &mut table.key_constraints {
        key.columns.iter_mut().for_each(rename);
    }
    for key in &mut table.foreign_keys {
        rename(&mut key.column);
    }
    Ok(())
}
