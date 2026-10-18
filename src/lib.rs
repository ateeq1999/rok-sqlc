//! Generate SQLx Rust database modules from SQL schema and query files.
//!
//! The stable library API is [`generate`] and the types it takes and
//! returns ([`Config`], [`Report`], [`Error`]). It does exactly what the
//! `rok-sqlc generate` command does, so a `build.rs` gets the same files and
//! the same failures as the CLI:
//!
//! ```no_run
//! // build.rs
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     println!("cargo::rerun-if-changed=migrations");
//!     let mut config = rok_sqlc::Config::default();
//!     config.schema = vec!["migrations/0001_init.sql".into()];
//!     config.output = "src/db".into();
//!     rok_sqlc::generate(&config)?;
//!     Ok(())
//! }
//! ```
//!
//! Anything else reachable from this crate is an implementation detail with
//! no stability promise (see ADR 0012).

mod api_config;
mod check_arch;
mod config;
mod config_file;
mod generate;
mod generator;
mod init;
mod output;
mod report;
mod schema;

// Implementation details, public only for the `rok-sqlc` binary (`cli`) and
// this crate's own tests (`parser`, `queries`). Not part of the API; they
// may change in any release.
#[doc(hidden)]
pub mod cli;
#[doc(hidden)]
pub mod parser;
#[doc(hidden)]
pub mod queries;

pub use api_config::{ApiAccess, ApiConfig, ApiEndpoint};
pub use config::{Config, Conventions, Derive, Extractor, IdGeneration, RelationshipChange};
pub use report::{Error, QueryFailure, RefusedFile, Report, Result, TableFailure};

/// Generates database modules as described by `config`.
///
/// On success the [`Report`] lists the files written, the files already up
/// to date, the refusals of a dry run, and the tables and queries skipped
/// under [`Config::lenient`]. Unsupported tables or queries (unless lenient)
/// and refused files fail the run before anything is written;
/// [`Error::report`] then says what was known when the run stopped.
///
/// After a successful run, stale files (generated files no longer in the
/// plan) are removed and listed in [`Report::stale`]. With
/// [`Config::check`] nothing is written or removed; any drift (missing,
/// changed, or stale files) fails the run, and the report lists it.
pub fn generate(config: &Config) -> Result<Report> {
    generate::generate(config)
}
