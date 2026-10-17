//! The PostgreSQL schema parser: reads schema inputs in order into one
//! `Schema`. The statement loop lives in `statements`; each kind of
//! statement it applies has a module of its own.

mod alter;
mod constraints;
mod enums;
mod names;
mod statements;
mod table;
#[cfg(test)]
mod tests;
mod triggers;
mod types;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::Result;

use self::table::table_failure;
use super::diagnostics::{LastGood, TableFailure};
use crate::schema::{Schema, Table};

pub(crate) use self::names::{ident_name, object_leaf_name};
pub use self::statements::parse_sources_with_modules;
pub(crate) use self::types::map_type;

pub struct ParsedSchema {
    pub schema: Schema,
    pub failures: Vec<TableFailure>,
    /// Where each table in `schema` was created (its `CREATE TABLE`), by
    /// module name, so a later check can fail it at that location.
    created: HashMap<String, (PathBuf, Option<usize>)>,
    /// The last statement that took a source of values away, by module name
    /// and column: a `DROP DEFAULT` of the column, or (with no column) the
    /// statement that left the table with no `BEFORE INSERT` trigger.
    source_removed: HashMap<(String, Option<String>), StatementLocation>,
    /// The `[modules]` keys that named no `CREATE TABLE` of the schema.
    pub unmatched_modules: Vec<String>,
}

/// Where a statement is: its file, its line there, and its position among
/// all statements, so the later of two statements can be told.
#[derive(Debug, Clone)]
struct StatementLocation {
    order: usize,
    path: PathBuf,
    line: Option<usize>,
}

/// Why a parsed table cannot be generated, found by a check after parsing.
#[derive(Debug, Clone)]
pub struct Rejection {
    pub reason: String,
    /// The managed column with no source of values, when that is the reason:
    /// the failure is then located at the statement that took the column's
    /// last source away, when a later statement did.
    pub unsourced_column: Option<String>,
}

impl ParsedSchema {
    /// Fails every table `check` rejects: the table leaves `schema.tables`
    /// and joins `failures`, located at its `CREATE TABLE`, or, for a managed
    /// column with no source of values, at the later of the `DROP DEFAULT`
    /// of that column and the statement that dropped the table's last
    /// `BEFORE INSERT` trigger.
    pub fn fail_tables(&mut self, mut check: impl FnMut(&Table) -> Option<Rejection>) {
        let mut failed = Vec::new();
        self.schema.tables.retain(|table| {
            let Some(rejection) = check(table) else {
                return true;
            };
            failed.push((table.clone(), rejection));
            false
        });
        for (table, rejection) in failed {
            let (mut path, mut line) = self.created.remove(&table.module_name).unwrap_or_default();
            if let Some(column) = &rejection.unsourced_column {
                let removed = |column: Option<String>| {
                    self.source_removed
                        .get(&(table.module_name.clone(), column))
                };
                let latest = [removed(Some(column.clone())), removed(None)]
                    .into_iter()
                    .flatten()
                    .max_by_key(|location| location.order);
                if let Some(location) = latest {
                    path.clone_from(&location.path);
                    line = location.line;
                }
            }
            self.failures.push(TableFailure {
                path: Some(path).filter(|path| !path.as_os_str().is_empty()),
                line,
                schema: table.schema.clone(),
                ..table_failure(&table.sql_name, rejection.reason)
            });
            // Its value lists are resolved, and their enums are among the
            // schema's.
            if let Some(failure) = self.failures.last_mut() {
                failure.last_good = Some(LastGood::new(table));
            }
        }
    }
}

/// Root modules rok-sqlc always generates next to the resource modules.
const GENERATED_MODULES: &[&str] = &["nullable", "schema", "search", "session", "types"];

/// One schema input: a migration file's path and its SQL.
pub struct SchemaSource<'a> {
    pub path: &'a Path,
    pub sql: &'a str,
}

/// Parses one schema text. Failures carry the line of the statement that
/// failed but no path.
pub fn parse_schema(sql: &str) -> Result<ParsedSchema> {
    let mut parsed = parse_sources(&[SchemaSource {
        path: Path::new(""),
        sql,
    }])?;
    for failure in &mut parsed.failures {
        failure.path = None;
    }
    Ok(parsed)
}

/// Parses schema inputs in order, as one schema. Each file is parsed on its
/// own, so every failure names its file and the line, within that file, of
/// the statement that failed: a table's `CREATE TABLE`, or the `ALTER TABLE`
/// it could not apply.
///
/// Later statements change earlier tables: `ALTER TABLE` adds, drops, and
/// renames columns and adds primary key, unique, foreign key, and value-list
/// check constraints, and `CREATE UNIQUE INDEX` on plain columns records a unique
/// key. Any other `ALTER TABLE` clause on a known table fails that table.
/// `CREATE TRIGGER` and `DROP TRIGGER` keep track of each table's row-level
/// `BEFORE INSERT` triggers, by name; the trigger functions are not read.
/// `DROP TABLE` removes a table, together with the foreign keys other tables
/// hold to it (as `CASCADE` would), and `DROP TYPE` removes an enum type.
/// Statements that leave the model unchanged (row-level security, policies,
/// non-unique indexes, grants, other triggers, comments, functions, `SELECT`
/// calls) and statements on unknown tables are ignored on purpose. Index,
/// sequence, and view names are tracked for default constraint names, and
/// an `ALTER TABLE` the SQL parser cannot read fails its table (see
/// `parse_statements`).
///
/// A thin wrapper over [`parse_sources_with_modules`] with no module
/// renames, kept for callers without a configuration: [`parse_schema`] and
/// the integration tests that parse the module corpus.
pub fn parse_sources(sources: &[SchemaSource<'_>]) -> Result<ParsedSchema> {
    parse_sources_with_modules(sources, &BTreeMap::new())
}

/// Whether a `[modules]` key names the table with this schema and SQL name:
/// the key is its schema-qualified name or its SQL name, and `public.` and
/// no schema agree.
pub fn names_table(key: &str, schema: Option<&str>, sql_name: &str) -> bool {
    let schema = schema.unwrap_or("public");
    key == sql_name
        || key
            .strip_suffix(sql_name)
            .and_then(|qualifier| qualifier.strip_suffix('.'))
            .is_some_and(|qualifier| qualifier == schema)
}
