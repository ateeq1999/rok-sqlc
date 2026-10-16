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
pub fn apply(tables: &mut [Table], conventions: &Conventions) {
    for table in tables {
        for column in &mut table.columns {
            column.is_managed = conventions.managed_columns.contains(&column.sql_name);
        }
    }
}
