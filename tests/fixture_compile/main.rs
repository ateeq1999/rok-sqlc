//! Generates the fixture schemas into the fixture crates and checks the
//! output the way a consuming crate would see it: `tests/fixture-app` (the
//! data layer), `tests/api-app` (the API features) and a project made with
//! `rok-sqlc init` and `rok-sqlc g`.
//!
//! Set `ROK_SQLC_TEST_DATABASE_URL` to a throwaway PostgreSQL database to
//! also execute the generated code (see `tests/fixture-app/tests/postgres/`).

#[path = "../common/mod.rs"]
mod common;

mod actix_template;
mod api_app;
mod api_app_axum;
mod axum_template;
mod fixture_app;
mod init;
mod init_axum;
mod template_project;

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, ensure};

/// Runs `rok-sqlc <args>` in `dir`, which must succeed, and returns its
/// stderr.
fn rok_sqlc(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
        .args(args)
        .current_dir(dir)
        .output()
        .context("failed to run rok-sqlc")?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    ensure!(
        output.status.success(),
        "rok-sqlc {args:?} failed:\n{}{stderr}",
        String::from_utf8_lossy(&output.stdout),
    );
    Ok(stderr)
}
