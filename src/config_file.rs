//! `rok-sqlc.toml`: the optional project configuration file the CLI reads.
//!
//! The file holds the same options as the command line. Without
//! `-c/--config`, the CLI looks for it in the current directory and then each
//! parent, like Cargo. Paths in the file resolve relative to the file's
//! directory. The library entry point never reads it (ADR 0011).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// The file name looked for during discovery.
pub(crate) const FILE_NAME: &str = "rok-sqlc.toml";

/// The options a config file sets, with paths already resolved against the
/// file's directory. `None` (or empty) means the file leaves the option to
/// flags and defaults.
#[derive(Debug, Default)]
pub(crate) struct FileConfig {
    pub(crate) schema: Vec<PathBuf>,
    pub(crate) queries: Vec<PathBuf>,
    pub(crate) output: Option<PathBuf>,
    pub(crate) resource: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    #[serde(default)]
    schema: Option<OneOrMany>,
    #[serde(default)]
    queries: Option<OneOrMany>,
    output: Option<PathBuf>,
    resource: Option<String>,
}

/// `schema = "a.sql"` and `schema = ["a.sql", "b.sql"]` are both accepted.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(PathBuf),
    Many(Vec<PathBuf>),
}

impl OneOrMany {
    fn into_vec(self) -> Vec<PathBuf> {
        match self {
            Self::One(path) => vec![path],
            Self::Many(paths) => paths,
        }
    }
}

/// The nearest `rok-sqlc.toml` in `start` or one of its parents.
pub(crate) fn discover(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(FILE_NAME))
        .find(|candidate| candidate.is_file())
}

/// Reads and validates the config file at `path`. Unknown keys are errors
/// naming the key and the file.
pub(crate) fn load(path: &Path) -> Result<FileConfig> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    let raw: RawFile =
        toml::from_str(&text).with_context(|| format!("invalid config file {}", path.display()))?;
    let base = path.parent().unwrap_or(Path::new(""));
    let resolve = |paths: Option<OneOrMany>| -> Vec<PathBuf> {
        paths
            .map(OneOrMany::into_vec)
            .unwrap_or_default()
            .into_iter()
            .map(|p| base.join(p))
            .collect()
    };
    Ok(FileConfig {
        schema: resolve(raw.schema),
        queries: resolve(raw.queries),
        output: raw.output.map(|p| base.join(p)),
        resource: raw.resource,
    })
}
