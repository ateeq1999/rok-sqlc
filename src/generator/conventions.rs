//! Conventions (ADR 0010): the configured column names, applied to the
//! tables being generated before any renderer runs.
//!
//! A convention is recorded on the schema model, so renderers ask a column
//! or table what it is (`Column::is_managed`) instead of each taking the
//! configuration. Only the tables generated in a run are annotated; query
//! files never see conventions.

use crate::Conventions;
use crate::schema::Table;

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
            column.is_managed = conventions.managed_columns.contains(&column.sql_name)
                || Some(column.sql_name.as_str()) == version;
        }
    }
}
