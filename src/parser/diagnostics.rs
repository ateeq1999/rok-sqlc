use std::fmt;
use std::path::PathBuf;

use crate::report::display_path;
use crate::schema::{EnumType, Table};

#[derive(Debug, Clone)]
pub struct TableFailure {
    pub table: String,
    pub reason: String,
    /// The schema file of the statement that failed the table, when parsed
    /// from one.
    pub path: Option<PathBuf>,
    /// 1-based line, in its file, of the statement that failed the table: its
    /// `CREATE TABLE`, or an `ALTER TABLE` that could not be applied.
    pub line: Option<usize>,
    /// The failed table's schema qualifier as written, when known, so a later
    /// `DROP TABLE` can forget the failure.
    pub schema: Option<String>,
    /// The table's last shape that generated, when rok-sqlc knows it, so an
    /// earlier run's files for it can be kept (ADR 0001).
    pub last_good: Option<Box<LastGood>>,
}

/// The shape a failed table had before the statement that failed it: the
/// shape an earlier run generated its files from, as long as the migrations
/// that run saw end before that statement. A table failed by a check after
/// parsing has its parsed shape. A table that failed at its `CREATE TABLE`,
/// or whose value lists cannot be generated, has none.
#[derive(Debug, Clone)]
pub struct LastGood {
    /// The table, its value lists resolved to enums.
    pub table: Table,
    /// The value-list enums of `table` that no generated table defines.
    pub enums: Vec<EnumType>,
}

impl LastGood {
    pub fn new(table: Table) -> Box<Self> {
        Box::new(Self {
            table,
            enums: Vec::new(),
        })
    }
}

impl fmt::Display for TableFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let (Some(path), Some(line)) = (&self.path, self.line) {
            write!(f, "{}:{line}: ", display_path(path))?;
        }
        write!(f, "table `{}`: {}", self.table, self.reason)
    }
}
