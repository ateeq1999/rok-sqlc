//! Relationships (ADR 0014): inferred from foreign keys over the tables
//! generated together, recorded on the schema model, and rendered as each
//! resource's `relationships.rs` of loaders.
//!
//! A loader takes already-fetched rows and loads one relationship for all of
//! them in a single `= ANY($n)` query, so "with" loading never becomes one
//! query per row. It returns a map keyed by the value the two tables join on
//! (the parent's key): one related row per key for a to-one relationship, a
//! list per key (empty when nothing relates) for a to-many one.

mod changes;
mod infer;
mod loaders;
mod many_to_many;

use std::collections::BTreeMap;

use self::changes::{apply_changes, check_names, location};
use self::infer::record;
use crate::RelationshipChange;
use crate::schema::{Column, Table};

pub use self::changes::check_changes;
pub use self::loaders::{render, resolve};
pub use self::many_to_many::{JoinPair, join_pair};

/// Infers every relationship among `tables` and records it on both of its
/// tables (`Table::relationships`). Runs after typed ids and conventions are
/// applied: the tenant column's foreign key makes no relationship.
///
/// A foreign key makes a relationship when the referenced table is among
/// `tables` and both columns have the same type, one a map key can hold.
///
/// `changes` (the configuration's `[relationships]`) then renames and opts
/// out relationships. Over the `Scope::Run` tables, entries for tables or
/// relationships outside the run are skipped, since [`check_changes`] has
/// already checked them against the whole schema, and the search filters'
/// field names are checked too. Returns the first name collision between two
/// relationships of a table, or the first invalid change.
pub fn infer(tables: &mut [Table], changes: &Changes, scope: Scope) -> Result<(), String> {
    record(tables);
    let renamed = apply_changes(tables, changes, scope == Scope::Schema)?;
    for table in tables.iter() {
        check_names(table, &renamed)?;
        if scope == Scope::Run {
            super::search::check_filter_fields(table, tables).map_err(|message| {
                format!(
                    "{message}; rename the relationship in {}",
                    location(&table.module_name, None)
                )
            })?;
        }
    }
    Ok(())
}

/// Which tables relationships are inferred over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The tables a run generates.
    Run,
    /// Every table of the schema, to check `[relationships]` entries.
    Schema,
}

/// The configuration's `[relationships]`: by table, then by inferred
/// relationship name.
pub type Changes = BTreeMap<String, BTreeMap<String, RelationshipChange>>;

fn find_column<'a>(table: &'a Table, name: &str) -> Option<&'a Column> {
    table.columns.iter().find(|column| column.sql_name == name)
}
