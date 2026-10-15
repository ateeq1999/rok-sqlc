use std::fmt;
use std::path::PathBuf;

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
}

impl fmt::Display for TableFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let (Some(path), Some(line)) = (&self.path, self.line) {
            write!(f, "{}:{line}: ", path.display())?;
        }
        write!(f, "table `{}`: {}", self.table, self.reason)
    }
}
