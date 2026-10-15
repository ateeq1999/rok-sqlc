use std::fmt;

#[derive(Debug, Clone)]
pub struct TableFailure {
    pub table: String,
    pub reason: String,
}

impl fmt::Display for TableFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "table `{}`: {}", self.table, self.reason)
    }
}
