use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct TableFailure {
    pub table: String,
    pub reason: String,
    /// The schema file the table's `CREATE TABLE` is in, when parsed from one.
    pub path: Option<PathBuf>,
    /// 1-based line of the table's `CREATE TABLE` in its file.
    pub line: Option<usize>,
}

impl fmt::Display for TableFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let (Some(path), Some(line)) = (&self.path, self.line) {
            write!(f, "{}:{line}: ", path.display())?;
        }
        write!(f, "table `{}`: {}", self.table, self.reason)
    }
}
