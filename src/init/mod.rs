//! `rok-sqlc init`: scaffolds the application skeleton of ADR 0016 into a
//! project, each file only when absent. It generates nothing; `rok-sqlc g`
//! does that once migrations exist.
//!
//! The skeleton refers to paths that generation fills in (ticket 04 fixed
//! them): `crate::features::{register_services, configure_all}` from the
//! generated `features/generated.rs`, `crate::shared::{health, pagination,
//! response, tenant, validation}`, and `crate::db::pool::{PoolSettings,
//! init_pool}`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::output::{SCAFFOLD_HEADER, write_absent};
use crate::report::display_path;

/// The skeleton: each file's path in the project and its text. Rust files
/// get [`SCAFFOLD_HEADER`] as their first line; the others carry it in
/// their own comment syntax.
const TEMPLATES: &[(&str, &str)] = &[
    ("AGENTS.md", include_str!("templates/AGENTS.md")),
    ("rok-sqlc.toml", include_str!("templates/rok-sqlc.toml")),
    ("src/app.rs", include_str!("templates/app.rs")),
    ("src/config.rs", include_str!("templates/config.rs")),
    ("src/error.rs", include_str!("templates/error.rs")),
    (
        "src/features/mod.rs",
        include_str!("templates/features/mod.rs"),
    ),
    ("src/main.rs", include_str!("templates/main.rs")),
    (
        "src/shared/auth.rs",
        include_str!("templates/shared/auth.rs"),
    ),
    ("src/shared/mod.rs", include_str!("templates/shared/mod.rs")),
];

/// The directory the migrations go in, created empty.
const MIGRATIONS: &str = "migrations";

/// The skeleton's files under `dir`, with their contents. Line endings are
/// `\n` whichever way the templates were checked out.
pub(crate) fn plan(dir: &Path) -> Vec<(PathBuf, String)> {
    TEMPLATES
        .iter()
        .map(|(path, text)| {
            let text = text.replace("\r\n", "\n");
            let contents = if path.ends_with(".rs") {
                format!("{SCAFFOLD_HEADER}\n\n{text}")
            } else {
                text
            };
            (dir.join(path), contents)
        })
        .collect()
}

/// Scaffolds the skeleton into `dir`, keeping every file that exists, and
/// creates `migrations/`. Returns the files written.
pub fn init(dir: &Path) -> Result<Vec<PathBuf>> {
    let migrations = dir.join(MIGRATIONS);
    fs::create_dir_all(&migrations)
        .with_context(|| format!("failed to create directory `{}`", display_path(&migrations)))?;
    write_absent(&plan(dir))
}

#[cfg(test)]
mod tests;
