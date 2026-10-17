//! `CREATE TABLE` and column definitions.

use std::collections::HashSet;

use sqlparser::ast::{ColumnDef, ColumnOption, CreateTable, TableConstraint};

use super::checks::{RangeCheck, apply_range_check, range_check};
use super::constraints::{add_key_constraint, constraint_foreign_key, key_columns};
use super::enums::{EnumRegistry, restrict_to_values, value_list_check};
use super::names::{ident_name, object_leaf_name, object_schema_name};
use super::types::{declared_length, is_serial_type, map_type_in};
use crate::parser::diagnostics::TableFailure;
use crate::parser::naming;
use crate::schema::{Column, ForeignKey, Table, UniqueKey};

pub(super) fn build_table(
    create: &CreateTable,
    enums: &EnumRegistry,
    taken: &HashSet<String>,
) -> std::result::Result<Table, Box<TableFailure>> {
    let sql_name = object_leaf_name(&create.name);
    let fail = |reason: String| Box::new(table_failure(&sql_name, reason));

    if create.columns.is_empty() {
        return Err(fail("table has no columns".to_owned()));
    }

    let mut raw_columns: Vec<RawColumn> = Vec::new();
    let mut seen_fields: HashSet<String> = HashSet::new();
    let mut foreign_keys = Vec::new();
    let mut unique_keys = Vec::new();
    // `(name, columns, primary)`, named once every constraint is known.
    let mut constraints: Vec<(Option<String>, Vec<String>, bool)> = Vec::new();
    let mut inline_primary_keys: Vec<String> = Vec::new();
    let mut value_lists: Vec<(String, Vec<String>)> = Vec::new();
    let mut range_checks: Vec<RangeCheck> = Vec::new();

    for column_def in &create.columns {
        let built = build_column(column_def, enums).map_err(fail)?;
        if !seen_fields.insert(built.raw.rust_name.clone()) {
            return Err(fail(field_taken(&built.raw)));
        }
        if let Some(name) = &built.primary_key {
            inline_primary_keys.push(built.raw.sql_name.clone());
            constraints.push((name.clone(), vec![built.raw.sql_name.clone()], true));
        }
        if let Some(name) = &built.unique {
            unique_keys.push(UniqueKey::new(vec![built.raw.sql_name.clone()]));
            constraints.push((name.clone(), vec![built.raw.sql_name.clone()], false));
        }
        foreign_keys.extend(built.foreign_key);
        value_lists.extend(built.value_lists);
        range_checks.extend(built.range_checks);
        raw_columns.push(built.raw);
    }

    if inline_primary_keys.len() > 1 {
        return Err(fail(format!(
            "multiple inline primary keys ({}) are not supported",
            inline_primary_keys.join(", ")
        )));
    }

    let mut table_primary_key: Vec<String> = Vec::new();
    for constraint in &create.constraints {
        match constraint {
            TableConstraint::PrimaryKey {
                name,
                columns: keys,
                ..
            } => {
                table_primary_key = key_columns(keys, "PRIMARY KEY").map_err(fail)?;
                let name = name.as_ref().map(ident_name);
                constraints.push((name, table_primary_key.clone(), true));
            }
            TableConstraint::Unique {
                name,
                columns: keys,
                ..
            } => {
                if let Ok(keys) = key_columns(keys, "UNIQUE") {
                    unique_keys.push(UniqueKey::new(keys.clone()));
                    constraints.push((name.as_ref().map(ident_name), keys, false));
                }
            }
            TableConstraint::Check { expr, .. } => {
                value_lists.extend(value_list_check(expr));
                range_checks.extend(range_check(expr));
            }
            other => foreign_keys.extend(constraint_foreign_key(other)),
        }
    }

    let primary_key = if table_primary_key.is_empty() {
        inline_primary_keys
    } else {
        table_primary_key
    };

    for key in &primary_key {
        if !raw_columns.iter().any(|raw| raw.sql_name == *key) {
            return Err(fail(format!(
                "primary key `{key}` does not match any column"
            )));
        }
    }

    let mut columns: Vec<Column> = raw_columns
        .into_iter()
        .map(|raw| {
            let is_primary_key = primary_key.contains(&raw.sql_name);
            raw.into_column(is_primary_key)
        })
        .collect();
    for (name, labels) in value_lists {
        if let Some(column) = columns.iter_mut().find(|column| column.sql_name == name) {
            restrict_to_values(column, labels);
        }
    }
    for check in range_checks {
        if let Some(column) = columns
            .iter_mut()
            .find(|column| column.sql_name == check.column)
        {
            apply_range_check(column, check);
        }
    }

    let module_name = naming::rust_identifier(&sql_name).map_err(fail)?;
    let struct_name = naming::struct_name(&module_name);

    let mut table = Table {
        schema: object_schema_name(&create.name),
        sql_name,
        module_name,
        struct_name,
        columns,
        primary_key,
        foreign_keys,
        unique_keys,
        key_constraints: Vec::new(),
        version_column: None,
        soft_delete_column: None,
        tenant_column: None,
        relationships: Vec::new(),
        before_insert_triggers: Vec::new(),
    };
    // Like PostgreSQL, one `CREATE TABLE` makes one constraint of several
    // over the same column list: a primary key if any of them is, named by
    // the first that has a name.
    let mut merged: Vec<(Option<String>, Vec<String>, bool)> = Vec::new();
    for (name, columns, primary) in constraints {
        match merged
            .iter_mut()
            .find(|(_, earlier, _)| *earlier == columns)
        {
            Some((earlier_name, _, earlier_primary)) => {
                *earlier_primary |= primary;
                if earlier_name.is_none() {
                    *earlier_name = name;
                }
            }
            None => merged.push((name, columns, primary)),
        }
    }
    for (name, columns, primary) in merged {
        add_key_constraint(&mut table, name, columns, primary, taken);
    }
    Ok(table)
}

/// A column definition from `CREATE TABLE` or `ALTER TABLE … ADD COLUMN`,
/// with the constraints declared inline on it.
pub(super) struct BuiltColumn {
    pub(super) raw: RawColumn,
    /// `Some` for an inline `PRIMARY KEY`, holding its declared name.
    pub(super) primary_key: Option<Option<String>>,
    /// `Some` for an inline `UNIQUE`, holding its declared name.
    pub(super) unique: Option<Option<String>>,
    pub(super) foreign_key: Option<ForeignKey>,
    /// Value-list `CHECK`s declared inline, by the column they name (usually
    /// this one, but PostgreSQL lets a column check name any column).
    pub(super) value_lists: Vec<(String, Vec<String>)>,
    /// Range `CHECK`s declared inline, which may also name any column.
    pub(super) range_checks: Vec<RangeCheck>,
}

pub(super) fn build_column(
    column_def: &ColumnDef,
    enums: &EnumRegistry,
) -> std::result::Result<BuiltColumn, String> {
    let column_name = ident_name(&column_def.name);
    let column_fail = |reason: String| format!("column `{column_name}`: {reason}");

    // A serial column is `NOT NULL` with a sequence default, on or off the
    // primary key.
    let mut is_generated = is_serial_type(&column_def.data_type);
    let mut has_default = is_generated;
    let mut nullable = !is_generated;
    let mut primary_key = None;
    let mut unique = None;
    let mut foreign_key = None;
    let mut value_lists = Vec::new();
    let mut range_checks = Vec::new();
    for option in &column_def.options {
        match &option.option {
            ColumnOption::NotNull => nullable = false,
            ColumnOption::Unique { is_primary, .. } => {
                let name = Some(option.name.as_ref().map(ident_name));
                if *is_primary {
                    primary_key = name;
                } else {
                    unique = name;
                }
            }
            ColumnOption::ForeignKey {
                foreign_table,
                referred_columns,
                ..
            } if referred_columns.len() == 1 => {
                foreign_key = Some(ForeignKey {
                    column: column_name.clone(),
                    referenced_schema: object_schema_name(foreign_table),
                    referenced_table: object_leaf_name(foreign_table),
                    referenced_column: ident_name(&referred_columns[0]),
                });
            }
            ColumnOption::Default(_) => has_default = true,
            ColumnOption::Check(expr) => {
                value_lists.extend(value_list_check(expr));
                range_checks.extend(range_check(expr));
            }
            ColumnOption::Generated { .. } | ColumnOption::Identity(_) => {
                has_default = true;
                is_generated = true;
            }
            _ => {}
        }
    }

    let (base_type, sqlx_type) = map_type_in(&column_def.data_type, enums).map_err(column_fail)?;
    let rust_name = naming::rust_identifier(&column_name).map_err(column_fail)?;

    Ok(BuiltColumn {
        raw: RawColumn {
            sql_name: column_name,
            rust_name,
            base_type,
            sqlx_type,
            declared_length: declared_length(&column_def.data_type),
            nullable,
            has_default,
            is_generated,
        },
        primary_key,
        unique,
        foreign_key,
        value_lists,
        range_checks,
    })
}

pub(super) fn field_taken(raw: &RawColumn) -> String {
    format!(
        "column `{}` maps to Rust field `{}`, which is already taken by another column",
        raw.sql_name, raw.rust_name
    )
}

pub(super) fn table_failure(table: &str, reason: String) -> TableFailure {
    TableFailure {
        table: table.to_owned(),
        reason,
        path: None,
        line: None,
        schema: None,
        last_good: None,
    }
}

pub(super) struct RawColumn {
    sql_name: String,
    pub(super) rust_name: String,
    base_type: String,
    sqlx_type: String,
    declared_length: Option<u32>,
    nullable: bool,
    has_default: bool,
    is_generated: bool,
}

impl RawColumn {
    pub(super) fn into_column(mut self, is_primary_key: bool) -> Column {
        if is_primary_key {
            self.nullable = false;
        }
        Column {
            rust_type: rust_type(&self.base_type, self.nullable),
            sql_name: self.sql_name,
            rust_name: self.rust_name,
            sqlx_type: self.sqlx_type,
            nullable: self.nullable,
            is_primary_key,
            has_default: self.has_default,
            is_generated: self.is_generated,
            value_list: None,
            declared_length: self.declared_length,
            value_range: None,
            length_range: None,
            typed_id: None,
            is_managed: false,
            id_generation: None,
        }
    }
}

pub(super) fn rust_type(base_type: &str, nullable: bool) -> String {
    if nullable {
        format!("Option<{base_type}>")
    } else {
        base_type.to_owned()
    }
}
