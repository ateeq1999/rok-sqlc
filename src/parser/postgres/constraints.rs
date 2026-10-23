//! Primary key, unique, foreign key, and check constraints added by
//! `ALTER TABLE`, and unique indexes.

use std::collections::HashSet;

use sqlparser::ast::{
    CreateIndex, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, IndexColumn,
    TableConstraint,
};

use super::alter::column_index;
use super::enums::any_array_as_in_list;
use super::named::{DeclaredCheck, DeclaredForeignKey, add_check, add_foreign_key};
use super::names::{
    default_constraint_name, find_table, ident_name, index_column_name, object_leaf_name,
    object_schema_name,
};
use super::table::rust_type;
use crate::schema::{ForeignKey, KeyConstraint, Table, UniqueKey};

/// Adds a primary key, unique, or foreign key constraint, or a value-list
/// or range check. Like in `CREATE TABLE`, any other check leaves the
/// columns unchanged; every check is recorded by name.
/// Other constraint kinds fail, so a constraint that might change the model
/// is never dropped silently.
pub(super) fn add_constraint(
    table: &mut Table,
    constraint: &TableConstraint,
    taken: &HashSet<String>,
    constraint_names: &HashSet<String>,
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
            if let Some(declared) = constraint_foreign_key(constraint) {
                add_foreign_key(table, declared, constraint_names);
            }
        }
        // Like in `CREATE TABLE`, a value list on a text column restricts it,
        // a range check narrows its column's ranges, and any other check
        // leaves the columns unchanged. Here a value list may also be in
        // `pg_dump`'s `col = ANY (ARRAY[…])` form; it is kept as `IN (…)`.
        TableConstraint::Check { name, expr, .. } => add_check(
            table,
            DeclaredCheck {
                name: name.as_ref().map(ident_name),
                expr: any_array_as_in_list(expr).unwrap_or_else(|| (**expr).clone()),
            },
            constraint_names,
        ),
        other => return Err(format!("unsupported ALTER TABLE clause `ADD {other}`")),
    }
    Ok(())
}

/// Records a `CREATE UNIQUE INDEX` on plain columns of a known table as a
/// unique key, keeping a partial index's condition. An index of plain
/// columns and exactly one `lower(<column>)` entry, such as
/// `(tenant_id, lower(name))`, makes a case-insensitive key comparing that
/// column ignoring case. Other expression indexes and non-unique indexes
/// change nothing.
pub(super) fn record_unique_index(tables: &mut [Table], index: &CreateIndex) {
    if !index.unique {
        return;
    }
    let Some(position) = find_table(tables, &index.table_name) else {
        return;
    };
    let mut columns = Vec::new();
    let mut lowered = None;
    for entry in &index.columns {
        if let Some(column) = index_column_name(entry) {
            columns.push(column);
        } else if let Some(column) = lowered_column(&entry.column.expr)
            && lowered.is_none()
        {
            lowered = Some(column.clone());
            columns.push(column);
        } else {
            return;
        }
    }
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
        lowered,
    });
}

/// The column of `lower(<column>)`: `lower` in any casing unless quoted,
/// the column optionally quoted or parenthesized.
fn lowered_column(expr: &Expr) -> Option<String> {
    let Expr::Function(function) = expr else {
        return None;
    };
    let [name] = function.name.0.as_slice() else {
        return None;
    };
    if ident_name(name.as_ident()?) != "lower"
        || function.filter.is_some()
        || function.over.is_some()
        || !function.within_group.is_empty()
        || !matches!(function.parameters, FunctionArguments::None)
    {
        return None;
    }
    let FunctionArguments::List(list) = &function.args else {
        return None;
    };
    let [FunctionArg::Unnamed(FunctionArgExpr::Expr(argument))] = list.args.as_slice() else {
        return None;
    };
    if list.duplicate_treatment.is_some() || !list.clauses.is_empty() {
        return None;
    }
    let mut argument = argument;
    while let Expr::Nested(inner) = argument {
        argument = inner;
    }
    match argument {
        Expr::Identifier(column) => Some(ident_name(column)),
        _ => None,
    }
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
    table.key_constraints.push(KeyConstraint {
        name,
        columns,
        primary,
    });
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

/// A `FOREIGN KEY` table constraint as declared. The model records a
/// single-column key; any other (a composite key, or one with no referenced
/// column) it keeps by name only.
pub(super) fn constraint_foreign_key(constraint: &TableConstraint) -> Option<DeclaredForeignKey> {
    let TableConstraint::ForeignKey {
        name,
        columns,
        foreign_table,
        referred_columns,
        ..
    } = constraint
    else {
        return None;
    };
    let key = (columns.len() == 1 && referred_columns.len() == 1).then(|| ForeignKey {
        name: String::new(),
        column: ident_name(&columns[0]),
        referenced_schema: object_schema_name(foreign_table),
        referenced_table: object_leaf_name(foreign_table),
        referenced_column: ident_name(&referred_columns[0]),
    });
    Some(DeclaredForeignKey {
        name: name.as_ref().map(ident_name),
        columns: columns.iter().map(ident_name).collect(),
        key,
    })
}
