//! `ON CONFLICT` targets and `ON CONSTRAINT` names, checked against the
//! table's keys.

use sqlparser::ast::{Ident, ObjectName};

use crate::parser::postgres::{ident_name, object_leaf_name};
use crate::schema::Table;

/// Checks that an `ON CONFLICT (cols)` target is one of the table's unique
/// keys (in any column order), so an upsert cannot fail at runtime for want
/// of a matching constraint. A partial index never matches: PostgreSQL infers
/// it only from a target with the index's `WHERE` condition, which query
/// files cannot express.
pub(super) fn check_conflict_target(table: &Table, target: &[Ident]) -> Result<(), String> {
    let target: Vec<String> = target.iter().map(ident_name).collect();
    let keys = table.unique_key_columns(None);
    let matches = |key: &Vec<String>| {
        key.len() == target.len() && target.iter().all(|column| key.contains(column))
    };
    if keys.iter().any(matches) {
        return Ok(());
    }
    let available = if keys.is_empty() {
        format!("`{}` has no unique keys", table.display_name())
    } else {
        let listed: Vec<String> = keys
            .iter()
            .map(|key| format!("({})", key.join(", ")))
            .collect();
        format!(
            "unique keys of `{}`: {}",
            table.display_name(),
            listed.join(", ")
        )
    };
    Err(format!(
        "ON CONFLICT ({}) matches no unique key; {available}",
        target.join(", ")
    ))
}

/// Checks that `ON CONFLICT ON CONSTRAINT name` names one of the table's
/// primary key or unique constraints. PostgreSQL accepts only a constraint
/// there, never a unique index.
pub(super) fn check_conflict_constraint(table: &Table, name: &ObjectName) -> Result<(), String> {
    let wanted = object_leaf_name(name);
    let names: Vec<&str> = table
        .key_constraints
        .iter()
        .map(|constraint| constraint.name.as_str())
        .collect();
    if names.contains(&wanted.as_str()) {
        return Ok(());
    }
    let available = if names.is_empty() {
        "it has none".to_owned()
    } else {
        format!("its constraints are {}", names.join(", "))
    };
    Err(format!(
        "ON CONFLICT ON CONSTRAINT {wanted} names no primary key or unique constraint of `{}`; {available}",
        table.display_name()
    ))
}
