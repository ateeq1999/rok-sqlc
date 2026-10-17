//! Stale-file removal and `generate --check`, driven through the CLI.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::Result;
use tempfile::TempDir;

#[path = "../common/mod.rs"]
mod common;

mod check;
mod lenient;
mod line_endings;
mod stale;

use common::shown;

const TWO_TABLES: &str = "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);\n\
     CREATE TABLE posts (id BIGSERIAL PRIMARY KEY, title TEXT NOT NULL);";
const USERS_ONLY: &str = "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);";

/// A schema file and an output directory in a temp dir.
struct Project {
    dir: TempDir,
}

impl Project {
    fn new(schema: &str) -> Result<Self> {
        let dir = TempDir::new()?;
        fs::write(dir.path().join("schema.sql"), schema)?;
        Ok(Self { dir })
    }

    fn set_schema(&self, schema: &str) -> Result<()> {
        fs::write(self.dir.path().join("schema.sql"), schema)?;
        Ok(())
    }

    fn out(&self) -> PathBuf {
        self.dir.path().join("out")
    }

    fn args<'a>(&'a self, extra: &[&'a str]) -> Vec<String> {
        let mut args = vec![
            "generate".to_owned(),
            "--schema".to_owned(),
            self.dir.path().join("schema.sql").display().to_string(),
            "--output".to_owned(),
            self.out().display().to_string(),
        ];
        args.extend(extra.iter().map(|arg| (*arg).to_owned()));
        args
    }

    fn generate(&self, extra: &[&str]) -> Result<()> {
        let mut full = vec!["rok-sqlc".to_owned()];
        full.extend(self.args(extra));
        rok_sqlc::cli::run_from(full)
    }

    fn binary(&self, extra: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
            .args(self.args(extra))
            .output()
            .expect("failed to spawn rok-sqlc binary")
    }

    fn tree(&self) -> Result<BTreeMap<PathBuf, String>> {
        read_tree(&self.out())
    }
}

/// Every file (with its contents) and directory under `root`, by relative path.
fn read_tree(root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, String>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let rel = path.strip_prefix(root)?.to_path_buf();
            if path.is_dir() {
                files.insert(rel, "<dir>".to_owned());
                walk(root, &path, files)?;
            } else {
                files.insert(rel, fs::read_to_string(&path)?);
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files)?;
    Ok(files)
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}
