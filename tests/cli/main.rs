use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::Result;
use tempfile::TempDir;

#[path = "../common/mod.rs"]
mod common;

mod collisions;
mod config;
mod derives;
mod flags;
mod generate;
mod json;
mod keys;
mod migrations;
mod modules;
mod queries;
mod runs;

use common::shown;

const SIMPLE_SCHEMA: &str = "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);";
const MIXED_SCHEMA: &str = "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);\n\
     CREATE TABLE spans (id BIGSERIAL PRIMARY KEY, duration INET NOT NULL);";

fn run(args: &[&str]) -> Result<()> {
    let mut full: Vec<String> = vec!["rok-sqlc".to_owned()];
    full.extend(args.iter().map(|arg| arg.to_string()));
    rok_sqlc::cli::run_from(full)
}

fn output_dir(dir: &TempDir) -> PathBuf {
    dir.path().join("out")
}

const QUERY_FILE: &str = "-- name: GetUserByEmail :maybe_one
SELECT id, email FROM users WHERE email = $1;

-- name: DeleteUser :execrows
DELETE FROM users WHERE id = $1;
";

fn run_binary(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
        .args(args)
        .output()
        .expect("failed to spawn rok-sqlc binary")
}

fn read_tree(root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, String>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                walk(root, &path, files)?;
            } else {
                let rel = path.strip_prefix(root)?.to_path_buf();
                files.insert(rel, fs::read_to_string(&path)?);
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files)?;
    Ok(files)
}

fn run_binary_in(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("failed to spawn rok-sqlc binary")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
