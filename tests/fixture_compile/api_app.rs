//! `tests/api-app`: an application laid out by `rok-sqlc init` and generated
//! with `api.output` (ADR 0016).

use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

use anyhow::{Context, Result, ensure};

use crate::common::{self, cargo};

/// The API fixture crate, `tests/api-app` (ADR 0016): an application laid
/// out by `rok-sqlc init` and generated with `api.output`.
pub(crate) fn api_app_dir() -> PathBuf {
    common::app_dir("api-app")
}

/// Every test regenerates into the API fixture crate, so they run one at a
/// time.
static API_APP: Mutex<()> = Mutex::new(());

/// Scaffolds the skeleton with `rok-sqlc init` into a scratch directory and
/// copies its `src/` files over the API fixture crate's, so every run
/// compiles the current templates. `lib.rs` is the exception: the fixture
/// crate's own declares the skeleton's modules too, under the lint gate.
/// Then generates `migrations/` into `src/db/`, `src/shared/` and
/// `src/features/`, and returns the generated file paths.
fn generate_api_app() -> Result<Vec<PathBuf>> {
    let scratch = tempfile::TempDir::new()?;
    // The fixture's package, so the scaffolded `main.rs` names its library.
    std::fs::copy(
        api_app_dir().join("Cargo.toml"),
        scratch.path().join("Cargo.toml"),
    )?;
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
    config.output = api_app_dir().join("src/db");
    config.force = true;
    // What `init`'s `rok-sqlc.toml` sets.
    config.derive = vec![rok_sqlc::Derive::Serde, rok_sqlc::Derive::Validate];
    config.extractors = vec![rok_sqlc::Extractor::ActixWeb];
    config.conventions.primary_key_ids = Some(rok_sqlc::IdGeneration::UuidV7);
    // The conventions: only `documents` has these columns.
    config.conventions.tenant_column = Some("tenant_id".into());
    config.conventions.version_column = Some("revision".into());
    config.conventions.soft_delete_column = Some("deleted_at".into());
    // A renamed relationship keeps its route under the new name:
    // `/notes/{id}/memberships`.
    config.relationships.insert(
        "notes".into(),
        [(
            "members".to_owned(),
            rok_sqlc::RelationshipChange::Rename("memberships".into()),
        )]
        .into(),
    );
    let mut api = rok_sqlc::ApiConfig::new(api_app_dir().join("src/features"));
    api.search_endpoint = true;
    let names = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect();
    api.search.insert("notes".into(), names(&["title", "body"]));
    api.search.insert("labels".into(), names(&["name"]));
    api.sort
        .insert("notes".into(), names(&["title", "priority", "created_at"]));
    api.skip
        .insert("labels".into(), vec![rok_sqlc::ApiEndpoint::Remove]);
    api.routes
        .insert("audit_events".into(), "/audit-log".to_owned());
    // Access: the shapes above are public, `accounts` is admin only, and
    // `tasks` keeps the default (`user`), owned through `owner_id`.
    for table in ["notes", "members", "audit_events", "labels", "drafts"] {
        api.access.insert(table.into(), rok_sqlc::ApiAccess::Public);
    }
    api.access
        .insert("accounts".into(), rok_sqlc::ApiAccess::Admin);
    api.hidden
        .insert("accounts".into(), names(&["password_hash"]));
    api.owner.insert("tasks".into(), "owner_id".to_owned());
    config.api = Some(api.clone());

    // The regeneration scenario: the features are generated from the first
    // migration, where `drafts` has no key, then from both, where it has
    // one. The fixture's own `drafts/service.rs` survives both runs as it
    // is checked in, and compiles against the trait the second run widens.
    let service = api_app_dir().join("src/features/drafts/service.rs");
    let checked_in = std::fs::read(&service).context("read drafts/service.rs")?;
    config.schema = vec![api_app_dir().join("migrations/0001_notes.sql")];
    let first = rok_sqlc::generate(&config)?;
    config.schema = vec![api_app_dir().join("migrations")];
    // `documents` (`0003_extended.sql`): admin only, owned and hiding
    // `secret`, so owner scoping works under `AdminUser` too.
    api.access
        .insert("documents".into(), rok_sqlc::ApiAccess::Admin);
    api.owner.insert("documents".into(), "owner_id".to_owned());
    api.hidden.insert("documents".into(), names(&["secret"]));
    // `gadgets` (`0004_gadgets.sql`): the column types the OpenAPI schemas
    // describe, public.
    api.access
        .insert("gadgets".into(), rok_sqlc::ApiAccess::Public);
    config.api = Some(api);
    let report = rok_sqlc::generate(&config)?;
    ensure!(
        std::fs::read(&service)? == checked_in,
        "regeneration changed the scaffolded drafts/service.rs"
    );
    for run in [&first, &report] {
        ensure!(
            !run.written.contains(&service) && !run.scaffolded.contains(&service),
            "regeneration wrote the scaffolded drafts/service.rs"
        );
    }
    // The Rust files: `README.md` is generated too.
    let files = report.written.into_iter().chain(report.unchanged);
    Ok(files
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect())
}

/// Copies the files under `from` to `to`, all but `from/lib.rs`.
fn copy_skeleton(from: &std::path::Path, to: &std::path::Path) -> Result<()> {
    for entry in std::fs::read_dir(from)? {
        let path = entry?.path();
        let target = to.join(path.file_name().context("a file name")?);
        if path.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_skeleton(&path, &target)?;
        } else if path != from.join("lib.rs") {
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

/// Generated code obeys the architecture it generates: the API fixture,
/// skeleton included, passes `rok-sqlc check-arch` (ADR 0016) with no
/// warning, reading the fixture's `rok-sqlc.toml`.
#[test]
fn api_app_passes_check_arch() -> Result<()> {
    let _guard = API_APP
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    generate_api_app()?;

    let output = Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
        .arg("check-arch")
        .current_dir(api_app_dir())
        .output()
        .context("failed to run rok-sqlc check-arch")?;
    ensure!(
        output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(),
        "the API fixture fails check-arch:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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
