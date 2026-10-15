//! `rok-sqlc.toml` and the `-j/--json` payload: the two configuration sources
//! the CLI reads besides its flags.
//!
//! Both hold the same options as the command line, in the same shape. Without
//! `-c/--config`, the CLI looks for `rok-sqlc.toml` in the current directory
//! and then each parent, like Cargo. Paths in the file resolve relative to the
//! file's directory; paths in a JSON payload resolve relative to the current
//! directory, like flags. The library entry point reads neither (ADR 0011).

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// The file name looked for during discovery.
pub(crate) const FILE_NAME: &str = "rok-sqlc.toml";

/// The options one configuration source sets, with paths already resolved.
/// `None` (or empty) means the source leaves the option to the sources below
/// it.
#[derive(Debug, Default)]
pub(crate) struct FileConfig {
    pub(crate) schema: Vec<PathBuf>,
    pub(crate) queries: Vec<PathBuf>,
    pub(crate) output: Option<PathBuf>,
    pub(crate) resource: Option<String>,
}

impl FileConfig {
    /// Layers `self` over `lower`: each option `self` sets wins, the rest come
    /// from `lower`. A list replaces the lower list whole; it never appends.
    pub(crate) fn over(self, lower: FileConfig) -> FileConfig {
        let list = |upper: Vec<PathBuf>, lower: Vec<PathBuf>| {
            if upper.is_empty() { lower } else { upper }
        };
        FileConfig {
            schema: list(self.schema, lower.schema),
            queries: list(self.queries, lower.queries),
            output: self.output.or(lower.output),
            resource: self.resource.or(lower.resource),
        }
    }
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

impl RawFile {
    /// Joins every path onto `base`.
    fn resolve(self, base: &Path) -> FileConfig {
        let resolve = |paths: Option<OneOrMany>| -> Vec<PathBuf> {
            paths
                .map(OneOrMany::into_vec)
                .unwrap_or_default()
                .into_iter()
                .map(|p| base.join(p))
                .collect()
        };
        FileConfig {
            schema: resolve(self.schema),
            queries: resolve(self.queries),
            output: self.output.map(|p| base.join(p)),
            resource: self.resource,
        }
    }
}

/// `schema = "a.sql"` and `schema = ["a.sql", "b.sql"]` are both accepted.
#[derive(Debug, Deserialize)]
#[serde(untagged, expecting = "expected a path or a list of paths")]
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
    Ok(raw.resolve(path.parent().unwrap_or(Path::new(""))))
}

/// Reads and validates a `-j/--json` argument: `-` reads the payload from
/// stdin, `@path` from a file, and anything else is the payload itself.
/// Paths in the payload stay relative to the current directory, wherever the
/// payload came from. Unknown keys and malformed JSON are errors naming the
/// source.
pub(crate) fn load_json(arg: &str) -> Result<FileConfig> {
    let (text, source) = if arg == "-" {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .context("failed to read the JSON payload from stdin")?;
        (text, "from stdin".to_owned())
    } else if let Some(path) = arg.strip_prefix('@') {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read JSON payload file {path}"))?;
        (text, format!("in {path}"))
    } else {
        (arg.to_owned(), "given to --json".to_owned())
    };
    let raw: RawFile =
        serde_json::from_str(&text).with_context(|| format!("invalid JSON payload {source}"))?;
    Ok(raw.resolve(Path::new("")))
}
