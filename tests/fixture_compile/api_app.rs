//! `tests/api-app`: an application laid out by `rok-sqlc init` and generated
//! with `api.output` (ADR 0016); and how both API fixtures, it and
//! `tests/api-app-axum`, are generated and checked.

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use anyhow::{Context, Result, ensure};
use rok_sqlc::ApiFramework;

use crate::common::{self, cargo};

/// The API fixture crate, `tests/api-app` (ADR 0016): an application laid
/// out by `rok-sqlc init` and generated with `api.output`.
pub(crate) fn api_app_dir() -> PathBuf {
    common::app_dir("api-app")
}

/// The API fixture crate for `framework`: `tests/api-app` for actix-web,
/// `tests/api-app-axum` for axum (0.7, decision A7).
pub(crate) fn fixture_dir(framework: ApiFramework) -> PathBuf {
    if framework == ApiFramework::Axum {
        common::app_dir("api-app-axum")
    } else {
        api_app_dir()
    }
}

/// Every test regenerates into its API fixture crate, so the tests of one
/// fixture run one at a time.
static API_APP: Mutex<()> = Mutex::new(());
/// [`API_APP`] for `tests/api-app-axum`.
static API_APP_AXUM: Mutex<()> = Mutex::new(());

/// Holds `framework`'s fixture crate for one test.
pub(crate) fn lock(framework: ApiFramework) -> MutexGuard<'static, ()> {
    let mutex = if framework == ApiFramework::Axum {
        &API_APP_AXUM
    } else {
        &API_APP
    };
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Scaffolds `framework`'s skeleton with `rok-sqlc init` into a scratch
/// directory and copies its `src/` files over the fixture crate's, so every
/// run compiles the current templates. `lib.rs` is the exception: the
/// fixture crate's own declares the skeleton's modules too, under the lint
/// gate. Then generates `tests/api-app/migrations/` into `src/db/`,
/// `src/shared/` and `src/features/` with the same settings for both
/// frameworks, and returns the generated file paths.
///
/// `features/drafts/service.rs` is a scaffolded service the fixture
/// changed: checked in under `tests/api-app`, and copied to
/// `tests/api-app-axum` first, as a service is the same for both
/// frameworks (decision A2).
fn generate_api_app(framework: ApiFramework) -> Result<Vec<PathBuf>> {
    let dir = fixture_dir(framework);
    let scratch = tempfile::TempDir::new()?;
    // The fixture's package, so the scaffolded `main.rs` names its library.
    std::fs::copy(dir.join("Cargo.toml"), scratch.path().join("Cargo.toml"))?;
    let output = Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
        .arg("init")
        .arg(scratch.path())
        .args(["--framework", framework.name()])
        .output()
        .context("failed to run rok-sqlc init")?;
    ensure!(
        output.status.success(),
        "rok-sqlc init failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    copy_skeleton(&scratch.path().join("src"), &dir.join("src"))?;

    let service = dir.join("src/features/drafts/service.rs");
    if framework != ApiFramework::Actix {
        std::fs::create_dir_all(dir.join("src/features/drafts"))?;
        std::fs::copy(
            api_app_dir().join("src/features/drafts/service.rs"),
            &service,
        )?;
    }

    let mut config = rok_sqlc::Config::default();
    config.output = dir.join("src/db");
    config.force = true;
    // What `init`'s `rok-sqlc.toml` sets.
    config.derive = vec![rok_sqlc::Derive::Serde, rok_sqlc::Derive::Validate];
    config.extractors = vec![framework.extractor()];
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
    let mut api = rok_sqlc::ApiConfig::new(dir.join("src/features"));
    api.framework = framework;
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
    // An account's email is set on create and never patched.
    api.immutable.insert("accounts".into(), names(&["email"]));
    api.owner.insert("tasks".into(), "owner_id".to_owned());
    config.api = Some(api.clone());

    // The regeneration scenario: the features are generated from the first
    // migration, where `drafts` has no key, then from both, where it has
    // one. The fixture's own `drafts/service.rs` survives both runs as it
    // is checked in, and compiles against the trait the second run widens.
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
    // `profiles` (`0006_profiles.sql`): a string rule or transform on each
    // column, public.
    api.access
        .insert("profiles".into(), rok_sqlc::ApiAccess::Public);
    api.rules.insert("profiles".into(), profile_rules());
    // `0008_scoped_checks.sql`: `folders` is owned.
    api.owner.insert("folders".into(), "owner_id".to_owned());
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

/// The string rules of `profiles`: one rule or transform per column
/// (`email` takes three), which `tests/api-suite/rules.rs` exercises.
fn profile_rules() -> std::collections::BTreeMap<String, Vec<rok_sqlc::ApiRule>> {
    use rok_sqlc::ApiRule;
    let text = |rules: &[&str]| rules.iter().map(|rule| ApiRule::from(*rule)).collect();
    [
        ("email", text(&["trim", "toLowerCase", "email"])),
        ("shout", text(&["toUpperCase"])),
        ("contact", text(&["normalizeEmail"])),
        ("website", text(&["url"])),
        ("ref_uuid", text(&["uuid(4)"])),
        ("ref_ulid", text(&["ulid"])),
        ("ip", text(&["ipAddress(4)"])),
        ("phone", text(&["mobile"])),
        ("first_name", text(&["alpha(allowSpaces)"])),
        ("handle", text(&["alphaNumeric(allowDashes)"])),
        ("plain", text(&["ascii"])),
        ("slug", vec![ApiRule::Regex("^[a-z0-9-]+$".into())]),
        ("country", text(&["in(SA, AE, EG)"])),
        (
            "username",
            vec![ApiRule::NotIn(vec!["admin".into(), "root".into()])],
        ),
        ("sku", text(&["startsWith(SKU-)"])),
        ("file_name", vec![ApiRule::EndsWith(".pdf".into())]),
        ("bio", text(&["minLength(3)", "maxLength(10)"])),
        ("pin", text(&["fixedLength(4)"])),
        ("color", text(&["hexCode"])),
        ("iban", text(&["iban"])),
        ("token", text(&["jwt"])),
        ("zip", text(&["postalCode(SA)"])),
        ("card", text(&["creditCard"])),
        ("location", text(&["coordinates"])),
    ]
    .into_iter()
    .map(|(column, rules)| (column.to_owned(), rules))
    .collect()
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

/// The skeleton `init` scaffolds for `framework` compiles against the
/// generated pool and shared tools, and all of it passes the lint gate (ADR
/// 0013), with `unwrap`, `expect` and `panic` denied.
pub(crate) fn passes_the_lint_gate(framework: ApiFramework) -> Result<()> {
    let _guard = lock(framework);
    let dir = fixture_dir(framework);
    if let Some(reason) = common::generated_code_skip_reason(&dir) {
        eprintln!("{reason}");
        return Ok(());
    }
    generate_api_app(framework)?;

    let status = cargo(
        &dir,
        &["clippy", "--quiet", "--all-targets", "--", "-Dwarnings"],
    )
    .status()
    .with_context(|| format!("failed to run cargo clippy in {}", common::shown(&dir)))?;
    ensure!(status.success(), "the API fixture failed the lint gate");
    Ok(())
}

/// The fixture's tests: health (against PostgreSQL when
/// `ROK_SQLC_TEST_DATABASE_URL` is set, and unreachable), the pool settings,
/// the shared tools, and the API suites of `tests/api-suite/`.
pub(crate) fn passes_its_tests(framework: ApiFramework) -> Result<()> {
    let _guard = lock(framework);
    let dir = fixture_dir(framework);
    if let Some(reason) = common::generated_code_skip_reason(&dir) {
        eprintln!("{reason}");
        return Ok(());
    }
    generate_api_app(framework)?;

    let status = cargo(&dir, &["test", "--quiet"])
        .status()
        .with_context(|| format!("failed to run cargo test in {}", common::shown(&dir)))?;
    ensure!(status.success(), "the API fixture failed its tests");
    Ok(())
}

/// Generated code obeys the architecture it generates: the fixture,
/// skeleton included, passes `rok-sqlc check-arch` (ADR 0016) with no
/// warning, reading the fixture's `rok-sqlc.toml`.
pub(crate) fn passes_check_arch(framework: ApiFramework) -> Result<()> {
    let _guard = lock(framework);
    let dir = fixture_dir(framework);
    generate_api_app(framework)?;

    let output = Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
        .arg("check-arch")
        .current_dir(&dir)
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

/// The generated code is rustfmt-clean.
pub(crate) fn is_rustfmt_clean(framework: ApiFramework) -> Result<()> {
    let _guard = lock(framework);
    let files = generate_api_app(framework)?;
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

#[test]
fn api_app_passes_the_lint_gate() -> Result<()> {
    passes_the_lint_gate(ApiFramework::Actix)
}

#[test]
fn api_app_passes_its_tests() -> Result<()> {
    passes_its_tests(ApiFramework::Actix)
}

#[test]
fn api_app_passes_check_arch() -> Result<()> {
    passes_check_arch(ApiFramework::Actix)
}

#[test]
fn api_app_generated_code_is_rustfmt_clean() -> Result<()> {
    is_rustfmt_clean(ApiFramework::Actix)
}
