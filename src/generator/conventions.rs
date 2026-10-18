//! Conventions (ADR 0010): the configured column names, applied to the
//! tables being generated before any renderer runs.
//!
//! A convention is recorded on the schema model, so renderers ask a column
//! or table what it is (`Column::is_managed`, `Table::soft_delete_column`,
//! `Table::tenant_column`) instead of each taking the configuration. Only the tables generated in a
//! run are annotated; query files never see conventions.

use crate::schema::{BaseType, Column, Table};
use crate::{Conventions, IdGeneration};

/// The search filter's switch for soft-deleted rows.
pub const INCLUDE_DELETED: &str = "include_deleted";

/// Marks, in each of `tables`, the columns `conventions` names.
///
/// The version column is recorded on the tables that have it, and is always
/// managed: the database advances it, so callers never write it.
pub fn apply(tables: &mut [Table], conventions: &Conventions) {
    let version = conventions.version_column.as_deref();
    for table in tables {
        table.version_column = version
            .filter(|name| table.columns.iter().any(|column| column.sql_name == *name))
            .map(str::to_owned);
        for column in &mut table.columns {
            column.is_managed = is_managed(&column.sql_name, conventions);
        }
        let present = |name: &Option<String>| {
            name.as_ref()
                .filter(|name| table.columns.iter().any(|column| column.sql_name == **name))
                .cloned()
        };
        let soft_delete_column = present(&conventions.soft_delete_column);
        let tenant_column = present(&conventions.tenant_column);
        table.soft_delete_column = soft_delete_column;
        table.tenant_column = tenant_column;
        if let Some(ids) = conventions.primary_key_ids {
            mark_generated_id(table, ids);
        }
    }
}

/// Marks the table's one primary key column as made by `ids` when the
/// strategy fits its type: `UUID` for every strategy but CUID2, text for a
/// ULID or a CUID2.
fn mark_generated_id(table: &mut Table, ids: IdGeneration) {
    let [key] = table.primary_key.as_slice() else {
        return;
    };
    let Some(column) = table
        .columns
        .iter_mut()
        .find(|column| column.sql_name == *key)
    else {
        return;
    };
    let fits = match column.base_type() {
        BaseType::Uuid => ids != IdGeneration::Cuid2,
        BaseType::Text => matches!(ids, IdGeneration::Ulid | IdGeneration::Cuid2),
        _ => false,
    };
    if fits && column.is_insertable() {
        column.id_generation = Some(ids);
    }
}

/// Whether `conventions` make the column named `name` managed: it is one of
/// the managed columns or the version column.
fn is_managed(name: &str, conventions: &Conventions) -> bool {
    conventions
        .managed_columns
        .iter()
        .any(|managed| managed == name)
        || conventions.version_column.as_deref() == Some(name)
}

/// The first managed column of `table` (as [`apply`] marked it) with no
/// source of values, and why: inserts leave a managed column out, so the
/// database must supply it, and a `NOT NULL` column with no `DEFAULT` (and
/// not serial, identity, or `GENERATED`) gets a value only from a
/// `BEFORE INSERT` trigger. The reason says how to fix it.
pub fn unsourced(table: &Table) -> Option<(&Column, String)> {
    if !table.before_insert_triggers.is_empty() {
        return None;
    }
    let column = table.columns.iter().find(|column| {
        column.is_managed && !column.nullable && !column.has_default && !column.is_generated
    })?;
    let reason = format!(
        "managed column `{}` is NOT NULL with no DEFAULT, and no BEFORE INSERT trigger is defined on the table, so inserts, which leave it out, have no value for it; add a DEFAULT, make the column nullable, or create a BEFORE INSERT trigger that sets it",
        column.sql_name
    );
    Some((column, reason))
}

/// Checks that each of `tables` can follow `conventions`: a soft-delete
/// column must be a nullable `TIMESTAMP` or `TIMESTAMPTZ`, and no column may
/// collide with the `include_deleted` switch it adds to the search filter.
/// Returns the first problem, naming the table and column.
pub fn check(tables: &[Table], conventions: &Conventions) -> Result<(), String> {
    let Some(name) = conventions.soft_delete_column.as_deref() else {
        return Ok(());
    };
    for table in tables {
        let Some(column) = table.columns.iter().find(|column| column.sql_name == name) else {
            continue;
        };
        let subject = format!(
            "soft-delete column `{}` of `{}`",
            column.sql_name,
            table.display_name()
        );
        if !is_timestamp(column) {
            return Err(format!(
                "{subject} must be a nullable TIMESTAMP or TIMESTAMPTZ, but its type maps to `{}`",
                column.sqlx_type
            ));
        }
        if !column.nullable {
            return Err(format!(
                "{subject} must be a nullable TIMESTAMP or TIMESTAMPTZ, but it is NOT NULL"
            ));
        }
        let collision = table
            .columns
            .iter()
            .find(|other| other.rust_name == INCLUDE_DELETED);
        if let Some(other) = collision {
            return Err(format!(
                "column `{}` of `{}` collides with the search filter's `{INCLUDE_DELETED}` switch, which the {subject} adds",
                other.sql_name,
                table.display_name()
            ));
        }
    }
    Ok(())
}

fn is_timestamp(column: &Column) -> bool {
    column.base_type().is_timestamp()
}
