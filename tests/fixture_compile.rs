//! Generates the fixture schema into `tests/fixture-app` and checks the
//! output the way a consuming crate would see it.
//!
//! Set `ROK_SQLC_TEST_DATABASE_URL` to a throwaway PostgreSQL database to
//! also execute the generated code (see `tests/fixture-app/tests/postgres.rs`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use anyhow::{Context, Result, ensure};

/// Every test regenerates into the same fixture crate, so they run one at a time.
static FIXTURE: Mutex<()> = Mutex::new(());

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn app_dir() -> PathBuf {
    manifest_dir().join("tests/fixture-app")
}

/// Generates the fixture schema into the fixture app and returns the
/// generated file paths.
fn generate_fixture() -> Result<Vec<PathBuf>> {
    let mut schema = String::new();
    for path in [
        manifest_dir().join("modules/inventory/schema.sql"),
        app_dir().join("extra_schema.sql"),
    ] {
        schema.push_str(
            &fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?,
        );
        schema.push('\n');
    }

    let parsed = rok_sqlc::parser::parse_schema(&schema)?;
    ensure!(
        parsed.failures.is_empty(),
        "fixture schema must be fully supported, got failures: {:?}",
        parsed.failures
    );

    let queries_path = app_dir().join("queries.sql");
    let queries = fs::read_to_string(&queries_path)
        .with_context(|| format!("failed to read {}", queries_path.display()))?;
    let (defs, mut query_failures) = rok_sqlc::queries::parse_query_file(&queries);
    let modules = parsed
        .schema
        .tables
        .iter()
        .map(|table| table.module_name.clone())
        .collect();
    let (analyzed, analysis_failures) = rok_sqlc::queries::analyze_queries(
        &defs,
        &parsed.schema.tables,
        &modules,
        &parsed.failures,
    );
    query_failures.extend(analysis_failures);
    ensure!(
        query_failures.is_empty(),
        "fixture queries must all be supported, got failures: {query_failures:?}"
    );

    let files = rok_sqlc::generator::plan_files_with_queries(
        &schema,
        &parsed.schema.tables,
        rok_sqlc::generator::QueriesModule::Generate(&analyzed),
        &app_dir().join("src/db"),
    );
    rok_sqlc::output::apply_files(&files, false, true)?;
    Ok(files.into_iter().map(|(path, _)| path).collect())
}

fn cargo(app_dir: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("cargo");
    command.args(args).current_dir(app_dir).env(
        "CARGO_TARGET_DIR",
        manifest_dir().join("target/fixture-target"),
    );
    command
}

#[test]
fn generated_code_compiles_without_warnings() -> Result<()> {
    let _guard = FIXTURE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    generate_fixture()?;

    // The fixture crate is `#![deny(warnings)]`; `--all-targets` also
    // type-checks the PostgreSQL integration tests.
    let status = cargo(&app_dir(), &["check", "--quiet", "--all-targets"])
        .status()
        .context("failed to run cargo check in tests/fixture-app")?;
    ensure!(status.success(), "generated code failed cargo check");
    Ok(())
}

#[test]
fn generated_code_is_rustfmt_clean() -> Result<()> {
    let _guard = FIXTURE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let files = generate_fixture()?;

    let output = Command::new("rustfmt")
        .args(["--edition", "2024", "--check"])
        .args(&files)
        .output()
        .context("failed to run rustfmt (install it with `rustup component add rustfmt`)")?;
    ensure!(
        output.status.success(),
        "generated code is not rustfmt-clean:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    Ok(())
}

#[test]
fn generated_code_passes_postgres_integration_tests() -> Result<()> {
    let _guard = FIXTURE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if std::env::var_os("ROK_SQLC_TEST_DATABASE_URL").is_none() {
        eprintln!(
            "skipping: set ROK_SQLC_TEST_DATABASE_URL to run the PostgreSQL integration tests"
        );
        return Ok(());
    }
    generate_fixture()?;

    let status = cargo(&app_dir(), &["test", "--quiet", "--test", "postgres"])
        .status()
        .context("failed to run cargo test in tests/fixture-app")?;
    ensure!(
        status.success(),
        "generated code failed the PostgreSQL integration tests"
    );
    Ok(())
}
