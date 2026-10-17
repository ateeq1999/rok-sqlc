//! Primary key, unique, foreign key, value-list check, and range check
//! constraints, and unique indexes.

use std::collections::HashSet;

use sqlparser::ast::{CreateIndex, IndexColumn, TableConstraint};

use super::alter::column_index;
use super::checks::{apply_range_check, range_check};
use super::enums::{restrict_to_values, value_list_check};
use super::names::{
    default_constraint_name, find_table, ident_name, index_column_name, object_leaf_name,
    object_schema_name,
};
use super::table::rust_type;
use crate::schema::{ForeignKey, KeyConstraint, Table, UniqueKey};

/// Adds a primary key, unique, or foreign key constraint, or a value-list
/// or range check. Like in `CREATE TABLE`, any other check leaves the model
/// unchanged.
/// Other constraint kinds fail, so a constraint that might change the model
/// is never dropped silently.
pub(super) fn add_constraint(
    table: &mut Table,
    constraint: &TableConstraint,
    taken: &HashSet<String>,
) -> std::result::Result<(), String> {
    let clause = |reason: String| format!("ADD CONSTRAINT: {reason}");
    let known_columns = |table: &Table, keys: &[String]| match keys
        .iter()
        .find(|key| column_index(table, key).is_none())
    {
        Some(key) => Err(clause(format!("column `{key}` does not exist"))),
        None => Ok(()),
    };
    match constraint {
        TableConstraint::PrimaryKey { name, columns, .. } => {
            let keys = key_columns(columns, "PRIMARY KEY").map_err(clause)?;
            if !table.primary_key.is_empty() {
                return Err(clause("the table already has a primary key".to_owned()));
            }
            known_columns(table, &keys)?;
            for column in &mut table.columns {
                if keys.contains(&column.sql_name) {
                    column.is_primary_key = true;
                    column.nullable = false;
                    column.rust_type = rust_type(&column.sqlx_type, false);
                }
            }
            add_key_constraint(
                table,
                name.as_ref().map(ident_name),
                keys.clone(),
                true,
                taken,
            );
            table.primary_key = keys;
        }
        TableConstraint::Unique { name, columns, .. } => {
            let keys = key_columns(columns, "UNIQUE").map_err(clause)?;
            known_columns(table, &keys)?;
            add_key_constraint(
                table,
                name.as_ref().map(ident_name),
                keys.clone(),
                false,
                taken,
            );
            table.unique_keys.push(UniqueKey::new(keys));
        }
        TableConstraint::ForeignKey { columns, .. } => {
            let keys: Vec<String> = columns.iter().map(ident_name).collect();
            known_columns(table, &keys)?;
            table
                .foreign_keys
                .extend(constraint_foreign_key(constraint));
        }
        // Like in `CREATE TABLE`, a value list on a text column restricts it,
        // a range check narrows its column's ranges, and any other check
        // leaves the model unchanged.
        TableConstraint::Check { expr, .. } => {
            if let Some((name, labels)) = value_list_check(expr)
                && let Some(index) = column_index(table, &name)
            {
                restrict_to_values(&mut table.columns[index], labels);
            }
            if let Some(check) = range_check(expr)
                && let Some(index) = column_index(table, &check.column)
            {
                apply_range_check(&mut table.columns[index], check);
            }
        }
        other => return Err(format!("unsupported ALTER TABLE clause `ADD {other}`")),
    }
    Ok(())
}

/// Records a `CREATE UNIQUE INDEX` on plain columns of a known table as a
/// unique key, keeping a partial index's condition. Expression indexes and
/// non-unique indexes change nothing.
pub(super) fn record_unique_index(tables: &mut [Table], index: &CreateIndex) {
    if !index.unique {
        return;
    }
    let Some(position) = find_table(tables, &index.table_name) else {
        return;
    };
    let Some(columns) = index
        .columns
        .iter()
        .map(index_column_name)
        .collect::<Option<Vec<String>>>()
    else {
        return;
    };
    let table = &mut tables[position];
    if columns
        .iter()
        .any(|column| column_index(table, column).is_none())
    {
        return;
    }
    table.unique_keys.push(UniqueKey {
        columns,
        predicate: index.predicate.as_ref().map(ToString::to_string),
    });
}

/// Records a primary key or unique constraint under its declared name, or
/// under the name PostgreSQL gives an unnamed one (see
/// [`default_constraint_name`]). `elsewhere` holds the relation names other
/// tables of the schema take (see [`relation_names`](super::names::relation_names)).
pub(super) fn add_key_constraint(
    table: &mut Table,
    name: Option<String>,
    columns: Vec<String>,
    primary: bool,
    elsewhere: &HashSet<String>,
) {
    let name = name.unwrap_or_else(|| {
        let taken = |candidate: &str| {
            candidate == table.sql_name
                || elsewhere.contains(candidate)
                || table
                    .key_constraints
                    .iter()
                    .any(|constraint| constraint.name == candidate)
        };
        default_constraint_name(&table.sql_name, &columns, primary, taken)
    });
    table.key_constraints.push(KeyConstraint { name, columns });
}

/// The plain column names of a `PRIMARY KEY` or `UNIQUE` column list.
pub(super) fn key_columns(
    keys: &[IndexColumn],
    what: &str,
) -> std::result::Result<Vec<String>, String> {
    let mut names = Vec::with_capacity(keys.len());
    for key in keys {
        let name = index_column_name(key)
            .ok_or_else(|| format!("{what} constraint contains a non-column expression"))?;
        if names.contains(&name) {
            return Err(format!("{what} lists column `{name}` more than once"));
        }
        names.push(name);
    }
    Ok(names)
}

/// A single-column `FOREIGN KEY` table constraint; composite keys are not
/// recorded.
pub(super) fn constraint_foreign_key(constraint: &TableConstraint) -> Option<ForeignKey> {
    match constraint {
        TableConstraint::ForeignKey {
            columns,
            foreign_table,
            referred_columns,
            ..
        } if columns.len() == 1 && referred_columns.len() == 1 => Some(ForeignKey {
            column: ident_name(&columns[0]),
            referenced_schema: object_schema_name(foreign_table),
            referenced_table: object_leaf_name(foreign_table),
            referenced_column: ident_name(&referred_columns[0]),
        }),
        _ => None,
    }
}
