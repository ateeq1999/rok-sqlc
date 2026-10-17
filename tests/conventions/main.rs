//! The `[conventions]` configuration section, through the CLI: the `rok`
//! preset, explicit keys, validation, and managed columns. The PostgreSQL
//! behavior of managed columns is proven in `tests/fixture-app`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use tempfile::TempDir;

#[path = "../common/mod.rs"]
mod common;

mod ids;
mod managed;
mod sources;
mod tenant;
mod versions;

use common::shown;

/// A table maintained by a `touch_row`-style trigger, one without any
/// convention column, and two with an `organization_id` tenant column (one
/// with it in the primary key).
const SCHEMA: &str = "CREATE TABLE documents (
    id UUID PRIMARY KEY,
    title TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    row_version BIGINT NOT NULL DEFAULT 0,
    deleted_at TIMESTAMPTZ,
    removed_at TIMESTAMPTZ
);
CREATE TABLE tags (id UUID PRIMARY KEY, label TEXT NOT NULL);
CREATE TABLE notes (
    id UUID PRIMARY KEY,
    organization_id UUID NOT NULL,
    code TEXT NOT NULL,
    body TEXT NOT NULL,
    UNIQUE (organization_id, code)
);
CREATE TABLE members (
    organization_id UUID NOT NULL,
    id UUID NOT NULL,
    email TEXT NOT NULL,
    PRIMARY KEY (organization_id, id)
);
";

fn run(args: &[&str]) -> Result<()> {
    let mut full: Vec<String> = vec!["rok-sqlc".to_owned()];
    full.extend(args.iter().map(|arg| arg.to_string()));
    rok_sqlc::cli::run_from(full)
}

/// A temp dir holding `schema.sql` and a `rok-sqlc.toml` that generates it
/// into `out/` with the given extra TOML appended.
fn project(extra_toml: &str) -> Result<(TempDir, PathBuf)> {
    project_with(SCHEMA, extra_toml)
}

/// `project`, generating `schema` instead of `SCHEMA`.
fn project_with(schema: &str, extra_toml: &str) -> Result<(TempDir, PathBuf)> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), schema)?;
    let config = dir.path().join("rok-sqlc.toml");
    fs::write(
        &config,
        format!("schema = \"schema.sql\"\noutput = \"out\"\n{extra_toml}"),
    )?;
    Ok((dir, config))
}

fn generate(config: &Path) -> Result<()> {
    run(&["g", "-c", config.to_str().unwrap()])
}

/// The field names of `pub struct {name}` in `source`, in order.
fn struct_fields(source: &str, name: &str) -> Vec<String> {
    let start = source
        .find(&format!("pub struct {name} {{"))
        .unwrap_or_else(|| panic!("no struct {name} in:\n{source}"));
    let body = &source[start..];
    let body = &body[body.find('{').unwrap() + 1..body.find("\n}").unwrap()];
    body.lines()
        .filter_map(|line| line.trim().strip_prefix("pub "))
        .map(|field| field.split(':').next().unwrap().to_owned())
        .collect()
}

fn models(dir: &TempDir, module: &str) -> Result<String> {
    resource_file(dir, module, "models.rs")
}

/// Every file under `root`, keyed by its path relative to `root`.
fn tree(root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, String>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                walk(root, &path, out)?;
            } else {
                out.insert(
                    path.strip_prefix(root)?.to_path_buf(),
                    fs::read_to_string(&path)?,
                );
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

fn resource_file(dir: &TempDir, module: &str, file: &str) -> Result<String> {
    Ok(fs::read_to_string(
        dir.path().join("out").join(module).join(file),
    )?)
}
