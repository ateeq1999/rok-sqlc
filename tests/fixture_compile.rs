//! Generates the fixture schema into `tests/fixture-app` and checks the
//! output the way a consuming crate would see it.
//!
//! Set `ROK_SQLC_TEST_DATABASE_URL` to a throwaway PostgreSQL database to
//! also execute the generated code (see `tests/fixture-app/tests/postgres.rs`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use anyhow::{Context, Result, ensure};

mod common;

/// Every test regenerates into the same fixture crate, so they run one at a time.
static FIXTURE: Mutex<()> = Mutex::new(());

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn app_dir() -> PathBuf {
    manifest_dir().join("tests/fixture-app")
}

/// Generates the fixture schema into the fixture app through the library
/// entry point, as a consuming crate's `build.rs` would, and returns the
/// generated file paths. Any unsupported table or query fails the run.
fn generate_fixture() -> Result<Vec<PathBuf>> {
    let mut config = rok_sqlc::Config::default();
    config.schema = vec![
        manifest_dir().join("modules/inventory/schema.sql"),
        app_dir().join("extra_schema.sql"),
        app_dir().join("alter_schema.sql"),
    ];
    // A directory: each query file becomes its own `db::queries::<stem>` module.
    config.queries = vec![app_dir().join("queries")];
    config.output = app_dir().join("src/db");
    config.force = true;
    // Only `touched_notes` has the managed columns and only `soft_notes` the
    // soft-delete column; every other table generates as if no convention
    // were configured.
    config.conventions.managed_columns = vec!["updated_at".into(), "row_version".into()];
    // Only `versioned_documents` has this column, so only its `update`
    // checks a version and returns an `UpdateOutcome`.
    config.conventions.version_column = Some("revision".into());
    config.conventions.soft_delete_column = Some("deleted_at".into());

    let report = rok_sqlc::generate(&config)?;
    Ok(report.written.into_iter().chain(report.unchanged).collect())
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
    if let Some(reason) = common::generated_code_skip_reason(&app_dir()) {
        eprintln!("{reason}");
        return Ok(());
    }
    generate_fixture()?;

    // The fixture crate is `#![deny(warnings)]`; `--all-targets` also
    // type-checks the PostgreSQL integration tests.
    let status = cargo(&app_dir(), &["check", "--quiet", "--all-targets"])
        .status()
        .context("failed to run cargo check in tests/fixture-app")?;
    ensure!(status.success(), "generated code failed cargo check");
    Ok(())
}

/// Typed ids make mixing up keys a compile error: the fixture crate's
/// `typed-id-mixup` feature adds code passing a `CategoryId` where a
/// `ProductId` is expected, and `cargo check` must reject exactly that.
#[test]
fn passing_another_tables_typed_id_fails_to_compile() -> Result<()> {
    let _guard = FIXTURE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(reason) = common::generated_code_skip_reason(&app_dir()) {
        eprintln!("{reason}");
        return Ok(());
    }
    generate_fixture()?;

    let output = cargo(
        &app_dir(),
        &["check", "--quiet", "--lib", "--features", "typed-id-mixup"],
    )
    .output()
    .context("failed to run cargo check in tests/fixture-app")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    ensure!(
        !output.status.success(),
        "a CategoryId was accepted where a ProductId is expected"
    );
    ensure!(
        stderr.contains("expected `ProductId`, found `CategoryId`")
            && stderr.contains("typed_id_mixup.rs"),
        "cargo check failed for another reason:\n{stderr}"
    );
    Ok(())
}

/// SQLx 0.9's `QueryBuilder` has no lifetime parameter, so naming one is a
/// compile error in the consuming crate. Runs on every toolchain, unlike the
/// compile checks.
#[test]
fn generated_code_names_no_query_builder_lifetime() -> Result<()> {
    let _guard = FIXTURE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let files = generate_fixture()?;
    ensure!(!files.is_empty(), "the fixture generated no files");
    for file in &files {
        let text = std::fs::read_to_string(file)?;
        ensure!(
            !text.contains("QueryBuilder<'"),
            "{} names a QueryBuilder lifetime",
            file.display()
        );
    }
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
    if let Some(reason) = common::generated_code_skip_reason(&app_dir()) {
        eprintln!("{reason}");
        return Ok(());
    }
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
