//! The module fixture harness: generates every module fixture under
//! `modules/` into the fixture crate `tests/modules-app`, then checks the
//! crate the way a consumer would see it.
//!
//! - A module with `migrations/` generates into `src/db/modules/<module>/`
//!   from those migrations (filename order, `*.down.sql` skipped).
//! - A module with a single-file `schema.sql` generates into
//!   `src/db/legacy_modules/<module>/` from it and the `queries.sql` written
//!   against it, where one exists.
//!
//! Every fixture generates leniently, so a table or query the generator
//! rejects is skipped rather than fatal; [`EXPECTED`] lists each rejection,
//! and any difference from it fails the harness.
//!
//! Set `ROK_SQLC_TEST_DATABASE_URL` to a PostgreSQL server to also apply
//! every fixture's migrations, in dependency order, to a fresh database and
//! smoke-run each generated table's reads (see
//! `tests/modules-app/tests/postgres.rs`). The URL's role must be able to
//! create databases; the database it names is not touched.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

use anyhow::{Context, Result, ensure};

use crate::expected::EXPECTED;
use crate::generate::generate_all;

#[path = "../common/mod.rs"]
mod common;

mod expected;
mod generate;
mod smoke;

use common::manifest_dir;

/// Every test regenerates into the same fixture crate, so they run one at a time.
static APP: Mutex<()> = Mutex::new(());

fn app_dir() -> PathBuf {
    common::app_dir("modules-app")
}

fn db_dir() -> PathBuf {
    app_dir().join("src/db")
}

fn cargo(args: &[&str]) -> Command {
    common::cargo(&app_dir(), args)
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    APP.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn module_fixtures_reject_exactly_the_expected_tables_and_queries() -> Result<()> {
    let _guard = lock();
    let outcomes = generate_all()?;

    for expected in EXPECTED {
        ensure!(
            outcomes
                .iter()
                .any(|outcome| outcome.fixture.id == expected.fixture),
            "EXPECTED lists `{}`, which is not a module fixture",
            expected.fixture
        );
    }

    let mut mismatches = Vec::new();
    for outcome in &outcomes {
        let id = &outcome.fixture.id;
        let expected = EXPECTED.iter().find(|expected| expected.fixture == *id);
        let tables = expected.map_or(&[][..], |expected| expected.tables);
        let queries = expected.map_or(&[][..], |expected| expected.queries);
        let functions = expected.map_or(&[][..], |expected| expected.functions);

        if !rejections_match(&outcome.tables, tables) {
            mismatches.push(format!(
                "`{id}`: expected rejected tables {tables:?}, got {:?}",
                outcome.tables
            ));
        }
        if !rejections_match(&outcome.queries, queries) {
            mismatches.push(format!(
                "`{id}`: expected rejected queries {queries:?}, got {:?}",
                outcome.queries
            ));
        }
        if outcome.functions != functions {
            mismatches.push(format!(
                "`{id}`: expected query functions {functions:?}, got {:?}",
                outcome.functions
            ));
        }
    }
    ensure!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    Ok(())
}

/// Whether `actual` rejections are exactly `expected`, in order, each
/// reason containing its expected fragment.
fn rejections_match(actual: &[(String, String)], expected: &[(&str, &str)]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|((name, reason), (expected_name, fragment))| {
                name == expected_name && reason.contains(fragment)
            })
}

#[test]
fn module_fixtures_compile_without_warnings() -> Result<()> {
    let _guard = lock();
    if let Some(reason) = common::generated_code_skip_reason(&app_dir()) {
        eprintln!("{reason}");
        return Ok(());
    }
    let outcomes = generate_all()?;
    ensure!(
        outcomes
            .iter()
            .filter(|outcome| outcome.fixture.migrations)
            .count()
            >= 25,
        "expected the 25-module migration corpus"
    );

    // The fixture crate denies warnings and `missing_docs` and turns on the
    // selected pedantic lints (ADR 0013); `--all-targets` also type-checks
    // the PostgreSQL smoke test.
    let status = cargo(&["clippy", "--quiet", "--all-targets", "--", "-Dwarnings"])
        .status()
        .context(
            "failed to run cargo clippy in tests/modules-app (install it with `rustup component add clippy`)",
        )?;
    ensure!(
        status.success(),
        "generated module fixtures failed the lint gate"
    );
    Ok(())
}

#[test]
fn module_fixtures_are_rustfmt_clean() -> Result<()> {
    let _guard = lock();
    let outcomes = generate_all()?;

    // One rustfmt run per fixture keeps the command line short enough for
    // Windows.
    let mut unformatted = Vec::new();
    for outcome in &outcomes {
        ensure!(
            !outcome.files.is_empty(),
            "fixture `{}` generated no files",
            outcome.fixture.id
        );
        let output = Command::new("rustfmt")
            .args(["--edition", "2024", "--check"])
            .args(&outcome.files)
            .output()
            .context("failed to run rustfmt (install it with `rustup component add rustfmt`)")?;
        if !output.status.success() {
            unformatted.push(format!(
                "`{}`:\n{}{}",
                outcome.fixture.id,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    ensure!(
        unformatted.is_empty(),
        "generated code is not rustfmt-clean:\n{}",
        unformatted.join("\n")
    );
    Ok(())
}

#[test]
fn module_fixtures_read_from_migrated_postgres() -> Result<()> {
    let _guard = lock();
    if let Some(reason) = common::generated_code_skip_reason(&app_dir()) {
        eprintln!("{reason}");
        return Ok(());
    }
    if std::env::var_os("ROK_SQLC_TEST_DATABASE_URL").is_none() {
        eprintln!(
            "skipping: set ROK_SQLC_TEST_DATABASE_URL to smoke-run the module fixtures against PostgreSQL"
        );
        return Ok(());
    }
    generate_all()?;

    let status = cargo(&["test", "--quiet", "--test", "postgres"])
        .status()
        .context("failed to run cargo test in tests/modules-app")?;
    ensure!(
        status.success(),
        "generated module fixtures failed against the migrated corpus"
    );
    Ok(())
}
