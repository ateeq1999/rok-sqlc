//! `rok-sqlc init`: scaffolds the application skeleton of ADR 0016 into a
//! project, each file only when absent. It generates nothing; `rok-sqlc g`
//! does that once migrations exist.
//!
//! The application is a library crate (`src/lib.rs` declares every module
//! `pub`) with a thin `src/main.rs` that starts it, so generated code the
//! application does not use yet never warns. The skeleton refers to paths
//! that generation fills in: `crate::features::{register_services,
//! configure_all}` from the generated `features/generated.rs`,
//! `crate::shared::{health, pagination, response, tenant, validation}`, and
//! `crate::db::pool::{PoolSettings, init_pool}`.

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
    ("src/lib.rs", include_str!("templates/lib.rs")),
    ("src/main.rs", include_str!("templates/main.rs")),
    (
        "src/shared/auth.rs",
        include_str!("templates/shared/auth.rs"),
    ),
    ("src/shared/mod.rs", include_str!("templates/shared/mod.rs")),
];

/// The directory the migrations go in, created empty.
const MIGRATIONS: &str = "migrations";

/// Where `main.rs` names the library crate.
const CRATE_PLACEHOLDER: &str = "{crate}";

/// Where `AGENTS.md` lists the rules `check-arch` enforces.
const RULES_PLACEHOLDER: &str = "{rules}";

/// The skeleton's files under `dir`, with their contents, for the library
/// crate `krate`. Line endings are `\n` whichever way the templates were
/// checked out.
pub(crate) fn plan(dir: &Path, krate: &str) -> Vec<(PathBuf, String)> {
    TEMPLATES
        .iter()
        .map(|(path, text)| {
            let text = text
                .replace("\r\n", "\n")
                .replace(CRATE_PLACEHOLDER, krate)
                .replace(RULES_PLACEHOLDER, &crate::check_arch::rules_markdown());
            let contents = if path.ends_with(".rs") {
                format!("{SCAFFOLD_HEADER}\n\n{text}")
            } else {
                text
            };
            (dir.join(path), contents)
        })
        .collect()
}

/// The library crate's name in `dir`: `Cargo.toml`'s `[lib] name`, else its
/// `[package] name` with `-` as `_`, else the directory's name made an
/// identifier (`app` when it cannot be).
fn crate_name(dir: &Path) -> String {
    let manifest = fs::read_to_string(dir.join("Cargo.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok());
    let declared = manifest.as_ref().and_then(|manifest| {
        ["lib", "package"].iter().find_map(|section| {
            manifest
                .get(*section)?
                .get("name")?
                .as_str()
                .map(|name| name.replace('-', "_"))
        })
    });
    if let Some(name) = declared {
        return name;
    }
    let name: String = fs::canonicalize(dir)
        .ok()
        .and_then(|dir| dir.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        name
    } else {
        "app".to_owned()
    }
}

/// What `init` did.
#[derive(Debug, Default)]
pub struct Scaffolded {
    /// The files written.
    pub written: Vec<PathBuf>,
    /// Whether `src/main.rs` was cargo's untouched default and was replaced.
    pub replaced_default_main: bool,
}

/// The `src/main.rs` `cargo new` writes. It is not the user's code, so the
/// scaffolded `main.rs` replaces it.
const CARGO_DEFAULT_MAIN: &str = "fn main() {\n    println!(\"Hello, world!\");\n}";

/// Whether `text` is [`CARGO_DEFAULT_MAIN`], whatever its line endings and
/// trailing newline.
fn is_cargo_default_main(text: &str) -> bool {
    text.replace("\r\n", "\n").trim_end_matches('\n') == CARGO_DEFAULT_MAIN
}

/// Scaffolds the skeleton into `dir`, keeping every file that exists but
/// cargo's untouched default `src/main.rs`, and creates `migrations/`.
pub fn init(dir: &Path) -> Result<Scaffolded> {
    let migrations = dir.join(MIGRATIONS);
    fs::create_dir_all(&migrations)
        .with_context(|| format!("failed to create directory `{}`", display_path(&migrations)))?;
    let main = dir.join("src/main.rs");
    let replaced_default_main =
        fs::read_to_string(&main).is_ok_and(|text| is_cargo_default_main(&text));
    if replaced_default_main {
        fs::remove_file(&main)
            .with_context(|| format!("failed to replace `{}`", display_path(&main)))?;
    }
    Ok(Scaffolded {
        written: write_absent(&plan(dir, &crate_name(dir)))?,
        replaced_default_main,
    })
}

#[cfg(test)]
mod tests;
