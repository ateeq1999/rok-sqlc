use std::fmt;
use std::path::PathBuf;

/// What a [`generate`](crate::generate) run did, or would do in a dry run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Report {
    /// Files written (in a dry run: files that would be written).
    pub written: Vec<PathBuf>,
    /// Files whose contents were already up to date.
    pub unchanged: Vec<PathBuf>,
    /// Existing files rok-sqlc did not generate and would not overwrite.
    pub refused: Vec<RefusedFile>,
    /// Unsupported tables: skipped when lenient, fatal otherwise.
    pub table_failures: Vec<TableFailure>,
    /// Unsupported queries: skipped when lenient, fatal otherwise.
    pub query_failures: Vec<QueryFailure>,
}

/// An existing file that generation would not overwrite.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RefusedFile {
    pub path: PathBuf,
    pub reason: String,
}

/// A table that cannot be generated.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TableFailure {
    /// The table's SQL name.
    pub table: String,
    pub reason: String,
}

impl fmt::Display for TableFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "table `{}`: {}", self.table, self.reason)
    }
}

/// A query that cannot be generated.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct QueryFailure {
    /// The query name, or `line N` when no valid name was annotated.
    pub query: String,
    pub reason: String,
}

impl fmt::Display for QueryFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "query `{}`: {}", self.query, self.reason)
    }
}

/// A failed [`generate`](crate::generate) run. Nothing was written.
///
/// [`Error::report`] holds what was known when the run stopped: the
/// unsupported tables and queries, or the refused files.
pub struct Error {
    inner: anyhow::Error,
    report: Box<Report>,
}

impl Error {
    pub(crate) fn new(inner: anyhow::Error, report: Report) -> Self {
        Self {
            inner,
            report: Box::new(report),
        }
    }

    /// What was known when the run stopped.
    pub fn report(&self) -> &Report {
        &self.report
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.inner, f)
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.inner.source()
    }
}

/// The result of a [`generate`](crate::generate) run.
pub type Result<T, E = Error> = std::result::Result<T, E>;
