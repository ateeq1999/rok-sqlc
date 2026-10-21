//! Checks and foreign keys by name: the names PostgreSQL gives unnamed
//! ones, `DROP CONSTRAINT`, and keeping what checks say about their columns
//! in step with the checks that remain.

use std::collections::HashSet;
use std::ops::ControlFlow;

use sqlparser::ast::{Expr, Ident, visit_expressions, visit_expressions_mut};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

use super::alter::column_index;
use super::checks::{apply_range_check, range_check};
use super::enums::{restrict_to_values, value_list_check};
use super::names::{default_check_or_foreign_key_name, ident_name, table_constraint_names};
use crate::schema::{ForeignKey, NamedConstraint, Table, UniqueKey};

/// A foreign key as declared, before it has its name.
pub(super) struct DeclaredForeignKey {
    pub(super) name: Option<String>,
    pub(super) columns: Vec<String>,
    /// The single-column key the model records (its `name` is set when it
    /// is added); `None` for a key the model keeps by name only.
    pub(super) key: Option<ForeignKey>,
}

/// A `CHECK` as declared, before it has its name.
pub(super) struct DeclaredCheck {
    pub(super) name: Option<String>,
    pub(super) expr: Expr,
}

/// Records a check under its name and applies what it says about its
/// column: a value list on a text column restricts it, and a range check
/// narrows its column's ranges. Any other check leaves the columns as they
/// are. `elsewhere` holds the constraint names other tables of the schema
/// take (see [`constraint_names`](super::names::constraint_names)).
pub(super) fn add_check(table: &mut Table, check: DeclaredCheck, elsewhere: &HashSet<String>) {
    let columns = named_columns(table, &check.expr);
    let name = choose_name(table, check.name, &columns, false, elsewhere);
    apply_check(table, &check.expr, None);
    table.named_constraints.push(NamedConstraint {
        name,
        columns,
        check: Some(check.expr.to_string()),
    });
}

/// Records a foreign key under its name: a single-column one among the
/// table's foreign keys, any other by name only.
pub(super) fn add_foreign_key(
    table: &mut Table,
    declared: DeclaredForeignKey,
    elsewhere: &HashSet<String>,
) {
    let name = choose_name(table, declared.name, &declared.columns, true, elsewhere);
    match declared.key {
        Some(key) => table.foreign_keys.push(ForeignKey { name, ..key }),
        None => table.named_constraints.push(NamedConstraint {
            name,
            columns: declared.columns,
            check: None,
        }),
    }
}

/// Applies `DROP CONSTRAINT`: a primary key leaves its columns as they are
/// (still `NOT NULL`), a unique constraint takes its unique key with it,
/// and a check no longer shapes its column, whose value list and ranges
/// come from the checks that remain. An unknown name fails, unless `IF
/// EXISTS`.
pub(super) fn drop_constraint(
    table: &mut Table,
    name: &str,
    if_exists: bool,
) -> std::result::Result<(), String> {
    if let Some(index) = position(&table.key_constraints, |key| key.name == name) {
        let key = table.key_constraints.remove(index);
        if key.primary {
            table.primary_key.clear();
            for column in &mut table.columns {
                column.is_primary_key = false;
            }
        } else {
            let unique = UniqueKey::new(key.columns);
            if let Some(index) = position(&table.unique_keys, |key| *key == unique) {
                table.unique_keys.remove(index);
            }
        }
    } else if let Some(index) = position(&table.foreign_keys, |key| key.name == name) {
        table.foreign_keys.remove(index);
    } else if let Some(index) = position(&table.named_constraints, |named| named.name == name) {
        let dropped = table.named_constraints.remove(index);
        if dropped.check.is_some() {
            reapply_checks(table, &dropped.columns);
        }
    } else if !if_exists {
        return Err(format!(
            "DROP CONSTRAINT: constraint `{name}` does not exist"
        ));
    }
    Ok(())
}

/// Applies `RENAME CONSTRAINT` to a constraint of any kind. An unknown name
/// changes nothing.
pub(super) fn rename_constraint(table: &mut Table, old: &str, new: &str) {
    let names = table
        .key_constraints
        .iter_mut()
        .map(|key| &mut key.name)
        .chain(table.foreign_keys.iter_mut().map(|key| &mut key.name))
        .chain(
            table
                .named_constraints
                .iter_mut()
                .map(|named| &mut named.name),
        );
    for name in names {
        if name == old {
            new.clone_into(name);
        }
    }
}

/// Follows a column rename in the named constraints, their check
/// expressions included.
pub(super) fn rename_column_in_constraints(table: &mut Table, old: &str, new: &str) {
    for named in &mut table.named_constraints {
        for column in &mut named.columns {
            if column == old {
                new.clone_into(column);
            }
        }
        let Some(text) = &mut named.check else {
            continue;
        };
        let Some(mut expr) = parse_check(text) else {
            continue;
        };
        let _ = visit_expressions_mut(&mut expr, |expr| {
            if let Expr::Identifier(ident) = expr
                && ident_name(ident) == old
            {
                *ident = Ident::with_quote('"', new);
            }
            ControlFlow::<()>::Continue(())
        });
        *text = expr.to_string();
    }
}

/// Like PostgreSQL, dropping a column drops the checks and composite
/// foreign keys that name it.
pub(super) fn drop_column_constraints(table: &mut Table, column: &str) {
    table
        .named_constraints
        .retain(|named| !named.columns.iter().any(|named| named == column));
}

/// The declared name, or the one PostgreSQL gives an unnamed check or
/// foreign key: no constraint of the schema has it.
fn choose_name(
    table: &Table,
    declared: Option<String>,
    columns: &[String],
    foreign_key: bool,
    elsewhere: &HashSet<String>,
) -> String {
    declared.unwrap_or_else(|| {
        let taken = |candidate: &str| {
            elsewhere.contains(candidate)
                || table_constraint_names(table).any(|name| name == candidate)
        };
        default_check_or_foreign_key_name(&table.sql_name, columns, foreign_key, taken)
    })
}

/// The table's columns `expr` names, in order of first use.
fn named_columns(table: &Table, expr: &Expr) -> Vec<String> {
    let mut columns: Vec<String> = Vec::new();
    let _ = visit_expressions(expr, |expr| {
        if let Expr::Identifier(ident) = expr {
            let name = ident_name(ident);
            if column_index(table, &name).is_some() && !columns.contains(&name) {
                columns.push(name);
            }
        }
        ControlFlow::<()>::Continue(())
    });
    columns
}

/// Applies a check's value list and range to their column, when it is one
/// of `only` (any column when `None`).
fn apply_check(table: &mut Table, expr: &Expr, only: Option<&[String]>) {
    let wanted = |column: &String| only.is_none_or(|only| only.contains(column));
    if let Some((name, labels)) = value_list_check(expr)
        && wanted(&name)
        && let Some(index) = column_index(table, &name)
    {
        restrict_to_values(&mut table.columns[index], labels);
    }
    if let Some(check) = range_check(expr)
        && wanted(&check.column)
        && let Some(index) = column_index(table, &check.column)
    {
        apply_range_check(&mut table.columns[index], check);
    }
}

/// Recomputes what checks say about `columns` from the checks that remain.
fn reapply_checks(table: &mut Table, columns: &[String]) {
    for column in columns {
        if let Some(index) = column_index(table, column) {
            let column = &mut table.columns[index];
            column.value_list = None;
            column.value_range = None;
            column.length_range = None;
        }
    }
    let remaining: Vec<Expr> = table
        .named_constraints
        .iter()
        .filter_map(|named| named.check.as_deref())
        .filter_map(parse_check)
        .collect();
    for expr in &remaining {
        apply_check(table, expr, Some(columns));
    }
}

fn parse_check(text: &str) -> Option<Expr> {
    Parser::new(&PostgreSqlDialect {})
        .try_with_sql(text)
        .ok()?
        .parse_expr()
        .ok()
}

fn position<T>(items: &[T], matches: impl Fn(&T) -> bool) -> Option<usize> {
    items.iter().position(matches)
}
