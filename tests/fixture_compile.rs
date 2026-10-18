//! Generates the fixture schema into `tests/fixture-app` and checks the
//! output the way a consuming crate would see it.
//!
//! Set `ROK_SQLC_TEST_DATABASE_URL` to a throwaway PostgreSQL database to
//! also execute the generated code (see `tests/fixture-app/tests/postgres/`).

use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

use anyhow::{Context, Result, ensure};
use rok_sqlc::RelationshipChange;

mod common;

use common::{cargo, manifest_dir};

/// Every test regenerates into the same fixture crate, so they run one at a time.
static FIXTURE: Mutex<()> = Mutex::new(());

fn app_dir() -> PathBuf {
    common::app_dir("fixture-app")
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
    // Each convention applies only to the tables with its column (named for
    // it in `extra_schema.sql`); every other table generates as if no
    // convention were configured.
    config.conventions.managed_columns = vec!["updated_at".into(), "row_version".into()];
    // Only `versioned_documents` and `tenant_notes` have this column, so only
    // their `update` checks a version and returns an `UpdateOutcome`.
    config.conventions.version_column = Some("revision".into());
    config.conventions.soft_delete_column = Some("deleted_at".into());
    // Only the `tenant_*` and `rls_notes` tables have the tenant column.
    config.conventions.tenant_column = Some("tenant_id".into());
    config.conventions.tenant_setting = Some("app.tenant_id".into());
    config.conventions.user_setting = Some("app.user_id".into());
    // Rows, ids, keys, enums, inputs, and patches derive serde, which the
    // PostgreSQL tests round-trip through JSON.
    config.derive = vec![rok_sqlc::Derive::Serde];
    // Renamed and opted-out relationships (see `expense_claims` in
    // `extra_schema.sql`).
    let claims = config
        .relationships
        .entry("expense_claims".into())
        .or_default();
    claims.insert(
        "created_by_user".into(),
        RelationshipChange::Rename("author".into()),
    );
    claims.insert("approved_by_user".into(), RelationshipChange::OptOut);
    config
        .relationships
        .entry("users".into())
        .or_default()
        .insert(
            "expense_claims_by_created_by_user".into(),
            RelationshipChange::Rename("authored_expense_claims".into()),
        );

    let report = rok_sqlc::generate(&config)?;
    let mut files: Vec<PathBuf> = report.written.into_iter().chain(report.unchanged).collect();

    // Types without serde support (`INTERVAL`, `TIME WITH TIME ZONE`,
    // `MONEY`, ranges) generate only without the serde derive, so they get a
    // second `db` module of their own. It also makes UUID keys in Rust.
    let mut plain = rok_sqlc::Config::default();
    plain.schema = vec![app_dir().join("plain_schema.sql")];
    plain.queries = vec![app_dir().join("plain_queries.sql")];
    plain.output = app_dir().join("src/plain_db");
    plain.force = true;
    plain.conventions.primary_key_ids = Some(rok_sqlc::IdGeneration::UuidV7);
    let report = rok_sqlc::generate(&plain)?;
    files.extend(report.written.into_iter().chain(report.unchanged));

    // Validation rules (ADR 0015) get a third `db` module, so the main one
    // stays as `derive = ["serde"]` generates it.
    let mut validated = rok_sqlc::Config::default();
    validated.schema = vec![app_dir().join("validated_schema.sql")];
    validated.output = app_dir().join("src/validated_db");
    validated.force = true;
    validated.derive = vec![rok_sqlc::Derive::Serde, rok_sqlc::Derive::Validate];
    // `db/http/`, whose extractors `tests/axum.rs` and `tests/actix_web.rs`
    // drive.
    validated.extractors = vec![rok_sqlc::Extractor::Axum, rok_sqlc::Extractor::ActixWeb];
    let report = rok_sqlc::generate(&validated)?;
    files.extend(report.written.into_iter().chain(report.unchanged));

    // CUID2 text keys get a fourth `db` module, so `plain_db` keeps making
    // UUID version 7 keys.
    let mut cuid = rok_sqlc::Config::default();
    cuid.schema = vec![app_dir().join("cuid_schema.sql")];
    cuid.output = app_dir().join("src/cuid_db");
    cuid.force = true;
    cuid.conventions.primary_key_ids = Some(rok_sqlc::IdGeneration::Cuid2);
    let report = rok_sqlc::generate(&cuid)?;
    files.extend(report.written.into_iter().chain(report.unchanged));
    Ok(files)
}

/// The rules on generated inputs and patches accept and reject the right
/// values, with the right codes and messages (`tests/validate.rs` in the
/// fixture crate). Needs no database.
#[test]
fn generated_validation_rules_check_inputs_and_patches() -> Result<()> {
    let _guard = FIXTURE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(reason) = common::generated_code_skip_reason(&app_dir()) {
        eprintln!("{reason}");
        return Ok(());
    }
    generate_fixture()?;

    let status = cargo(&app_dir(), &["test", "--quiet", "--test", "validate"])
        .status()
        .context("failed to run cargo test in tests/fixture-app")?;
    ensure!(
        status.success(),
        "generated validation rules failed their tests"
    );
    Ok(())
}

/// The generated axum extractor takes valid bodies and answers every
/// rejection with the wire standard's error body (`tests/axum.rs` in the
/// fixture crate). Needs no database.
#[test]
fn generated_axum_extractor_validates_bodies() -> Result<()> {
    let _guard = FIXTURE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(reason) = common::generated_code_skip_reason(&app_dir()) {
        eprintln!("{reason}");
        return Ok(());
    }
    generate_fixture()?;

    let status = cargo(&app_dir(), &["test", "--quiet", "--test", "axum"])
        .status()
        .context("failed to run cargo test in tests/fixture-app")?;
    ensure!(
        status.success(),
        "the generated axum extractor failed its tests"
    );
    Ok(())
}

/// The generated actix-web extractor takes valid bodies, answers every
/// rejection with the wire standard's error body, and answers each input with the
/// same bytes as the axum one beside it (`tests/actix_web.rs` in the
/// fixture crate). Needs no database.
#[test]
fn generated_actix_web_extractor_validates_bodies_like_axum() -> Result<()> {
    let _guard = FIXTURE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(reason) = common::generated_code_skip_reason(&app_dir()) {
        eprintln!("{reason}");
        return Ok(());
    }
    generate_fixture()?;

    let status = cargo(&app_dir(), &["test", "--quiet", "--test", "actix_web"])
        .status()
        .context("failed to run cargo test in tests/fixture-app")?;
    ensure!(
        status.success(),
        "the generated actix-web extractor failed its tests"
    );
    Ok(())
}

#[test]
fn generated_code_passes_the_lint_gate() -> Result<()> {
    let _guard = FIXTURE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(reason) = common::generated_code_skip_reason(&app_dir()) {
        eprintln!("{reason}");
        return Ok(());
    }
    generate_fixture()?;

    // The fixture crate denies warnings and `missing_docs` and turns on the
    // selected pedantic lints (ADR 0013); `--all-targets` also type-checks
    // the PostgreSQL integration tests.
    let status = cargo(
        &app_dir(),
        &["clippy", "--quiet", "--all-targets", "--", "-Dwarnings"],
    )
    .status()
    .context(
        "failed to run cargo clippy in tests/fixture-app (install it with `rustup component add clippy`)",
    )?;
    ensure!(status.success(), "generated code failed the lint gate");
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

/// The API fixture crate, `tests/api-app` (ADR 0016): an application laid
/// out by `rok-sqlc init` and generated with `api.output`.
fn api_app_dir() -> PathBuf {
    common::app_dir("api-app")
}

/// Every test regenerates into the API fixture crate, so they run one at a
/// time.
static API_APP: Mutex<()> = Mutex::new(());

/// Scaffolds the skeleton with `rok-sqlc init` into a scratch directory and
/// copies its `src/` files over the API fixture crate's, so every run
/// compiles the current templates. `main.rs` is the exception: the fixture
/// crate's own declares the skeleton's modules in `lib.rs`, where its tests
/// reach them. Then generates `migrations/` into `src/db/`, `src/shared/`
/// and `src/features/`, and returns the generated file paths.
fn generate_api_app() -> Result<Vec<PathBuf>> {
    let scratch = tempfile::TempDir::new()?;
    let output = Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
        .arg("init")
        .arg(scratch.path())
        .output()
        .context("failed to run rok-sqlc init")?;
    ensure!(
        output.status.success(),
        "rok-sqlc init failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    copy_skeleton(&scratch.path().join("src"), &api_app_dir().join("src"))?;

    let mut config = rok_sqlc::Config::default();
    config.schema = vec![api_app_dir().join("migrations")];
    config.output = api_app_dir().join("src/db");
    config.force = true;
    // What `init`'s `rok-sqlc.toml` sets.
    config.derive = vec![rok_sqlc::Derive::Serde, rok_sqlc::Derive::Validate];
    config.extractors = vec![rok_sqlc::Extractor::ActixWeb];
    config.conventions.primary_key_ids = Some(rok_sqlc::IdGeneration::UuidV7);
    config.api = Some(rok_sqlc::ApiConfig::new(api_app_dir().join("src/features")));
    let report = rok_sqlc::generate(&config)?;
    Ok(report.written.into_iter().chain(report.unchanged).collect())
}

/// Copies the files under `from` to `to`, all but `from/main.rs`.
fn copy_skeleton(from: &std::path::Path, to: &std::path::Path) -> Result<()> {
    for entry in std::fs::read_dir(from)? {
        let path = entry?.path();
        let target = to.join(path.file_name().context("a file name")?);
        if path.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_skeleton(&path, &target)?;
        } else if path != from.join("main.rs") {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// The skeleton `init` scaffolds compiles against the generated pool and
/// shared tools, and all of it passes the lint gate (ADR 0013), with
/// `unwrap`, `expect` and `panic` denied.
#[test]
fn api_app_passes_the_lint_gate() -> Result<()> {
    let _guard = API_APP
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(reason) = common::generated_code_skip_reason(&api_app_dir()) {
        eprintln!("{reason}");
        return Ok(());
    }
    generate_api_app()?;

    let status = cargo(
        &api_app_dir(),
        &["clippy", "--quiet", "--all-targets", "--", "-Dwarnings"],
    )
    .status()
    .context("failed to run cargo clippy in tests/api-app")?;
    ensure!(status.success(), "the API fixture failed the lint gate");
    Ok(())
}

/// The API fixture's tests: health (against PostgreSQL when
/// `ROK_SQLC_TEST_DATABASE_URL` is set, and unreachable), the pool settings,
/// and the shared tools.
#[test]
fn api_app_passes_its_tests() -> Result<()> {
    let _guard = API_APP
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(reason) = common::generated_code_skip_reason(&api_app_dir()) {
        eprintln!("{reason}");
        return Ok(());
    }
    generate_api_app()?;

    let status = cargo(&api_app_dir(), &["test", "--quiet"])
        .status()
        .context("failed to run cargo test in tests/api-app")?;
    ensure!(status.success(), "the API fixture failed its tests");
    Ok(())
}

#[test]
fn api_app_generated_code_is_rustfmt_clean() -> Result<()> {
    let _guard = API_APP
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let files = generate_api_app()?;
    ensure!(!files.is_empty(), "the API fixture generated no files");

    let output = Command::new("rustfmt")
        .args(["--edition", "2024", "--check"])
        .args(&files)
        .output()
        .context("failed to run rustfmt (install it with `rustup component add rustfmt`)")?;
    ensure!(
        output.status.success(),
        "generated API code is not rustfmt-clean:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    Ok(())
}
