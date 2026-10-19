//! `templates/actix-api`, made the way a user makes it: `rok-sqlc init
//! --template actix` into a scratch directory, then `rok-sqlc g` there. The
//! generated files the template checks in must be exactly what `g` writes
//! (set `ROK_SQLC_UPDATE_EXPECTED=1` to rewrite them), and the project must
//! pass its own checks: rustfmt, the lint gate, `check-arch`, and `cargo
//! test` (its contract test, and its API tests against
//! `ROK_SQLC_TEST_DATABASE_URL` when set).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, ensure};

use crate::common::{self, cargo};
use crate::rok_sqlc;

/// The template's directory.
fn template_dir() -> PathBuf {
    common::manifest_dir().join("templates/actix-api")
}

/// A project made from the template: `init --template actix` (which copies
/// the template's `Cargo.lock`, so builds resolve as the template's do),
/// then `g`.
fn materialize() -> Result<tempfile::TempDir> {
    let project = tempfile::TempDir::new()?;
    let dir = project.path();
    rok_sqlc(dir, &["init", "--template", "actix"])?;
    rok_sqlc(dir, &["g"])?;
    Ok(project)
}

/// rok-sqlc's generated files under `root` (a build's `target/` aside), by relative path, with `\n` line endings.
fn generated_files(root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, String>) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            let relative = path.strip_prefix(root)?.to_path_buf();
            if path.is_dir() {
                if relative != Path::new("target") {
                    walk(root, &path, files)?;
                }
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if common::is_generated(&text) {
                files.insert(relative, text.replace("\r\n", "\n"));
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files)?;
    Ok(files)
}

/// The template checks in exactly the files `rok-sqlc g` generates from its
/// schema and `rok-sqlc.toml`. With `ROK_SQLC_UPDATE_EXPECTED` set, it
/// rewrites them instead.
#[test]
fn actix_template_checks_in_what_generation_writes() -> Result<()> {
    let project = materialize()?;
    let generated = generated_files(project.path())?;
    ensure!(
        generated.contains_key(Path::new("src/features/users/dto.rs")),
        "`rok-sqlc g` generated no users feature: {:?}",
        generated.keys()
    );
    let checked_in = generated_files(&template_dir())?;
    if std::env::var_os("ROK_SQLC_UPDATE_EXPECTED").is_some() {
        for path in checked_in
            .keys()
            .filter(|path| !generated.contains_key(*path))
        {
            std::fs::remove_file(template_dir().join(path))?;
        }
        for (path, text) in &generated {
            let target = template_dir().join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(target, text)?;
        }
        return Ok(());
    }
    let drift: Vec<String> = generated
        .keys()
        .chain(checked_in.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|path| generated.get(*path) != checked_in.get(*path))
        .map(
            |path| match (generated.contains_key(path), checked_in.contains_key(path)) {
                (true, false) => format!("{}: generated, not checked in", path.display()),
                (false, true) => format!("{}: checked in, no longer generated", path.display()),
                _ => format!("{}: differs", path.display()),
            },
        )
        .collect();
    ensure!(
        drift.is_empty(),
        "templates/actix-api drifted from its generation (regenerate with ROK_SQLC_UPDATE_EXPECTED=1):\n  {}",
        drift.join("\n  ")
    );
    Ok(())
}

/// The template's README lists the dependencies and lints of its
/// `Cargo.toml`, for a project that keeps its own manifest.
#[test]
fn actix_template_readme_lists_its_manifest() -> Result<()> {
    let manifest = std::fs::read_to_string(template_dir().join("Cargo.toml.template"))?;
    let readme = std::fs::read_to_string(template_dir().join("README.md"))?.replace("\r\n", "\n");
    let listed: String = manifest
        .replace("\r\n", "\n")
        .lines()
        .skip_while(|line| *line != "[dependencies]")
        .filter(|line| !line.starts_with('#'))
        .map(|line| format!("{line}\n"))
        .collect();
    ensure!(
        readme.contains(&format!("```toml\n{}\n```", listed.trim_end())),
        "README.md's dependency block differs from Cargo.toml.template's:\n{listed}"
    );
    Ok(())
}

/// `cargo <args>` in `dir`, which must succeed without a warning.
fn cargo_clean(dir: &Path, args: &[&str], database: Option<&str>) -> Result<()> {
    let mut command = cargo(dir, args);
    match database {
        Some(url) => command.env("DATABASE_URL", url),
        None => command.env_remove("DATABASE_URL"),
    };
    let output = command.output().context("failed to run cargo")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    ensure!(
        output.status.success() && !stderr.contains("warning:"),
        "`cargo {}` fails in the actix template:\n{}{stderr}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout)
    );
    Ok(())
}

/// The project made from the template is rustfmt-clean, passes `cargo
/// clippy --all-targets -- -D warnings` and `check-arch`, and passes its
/// tests: the contract, and the API tests against PostgreSQL when
/// `ROK_SQLC_TEST_DATABASE_URL` is set (they skip otherwise); and started
/// unconfigured, it fails readably.
#[test]
fn actix_template_passes_its_checks_and_tests() -> Result<()> {
    let project = materialize()?;
    let dir = project.path();
    if let Some(reason) = common::generated_code_skip_reason(dir) {
        eprintln!("{reason}");
        return Ok(());
    }
    rok_sqlc(dir, &["check-arch"])?;
    let output = Command::new("cargo")
        .args(["fmt", "--check"])
        .current_dir(dir)
        .output()
        .context("failed to run cargo fmt")?;
    ensure!(
        output.status.success(),
        "the actix template is not rustfmt-clean:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    cargo_clean(
        dir,
        &["clippy", "--quiet", "--all-targets", "--", "-D", "warnings"],
        None,
    )?;
    let database = std::env::var("ROK_SQLC_TEST_DATABASE_URL").ok();
    cargo_clean(dir, &["test", "--quiet"], database.as_deref())?;
    starts_only_when_configured(dir)
}

/// Started without its settings, the server says which one is missing in
/// one readable line, no `Debug` dump, and exits non-zero.
fn starts_only_when_configured(dir: &Path) -> Result<()> {
    let output = cargo(dir, &["run", "--quiet"])
        .env_remove("DATABASE_URL")
        .env_remove("JWT_SECRET")
        .output()
        .context("failed to run cargo run")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    ensure!(
        !output.status.success()
            && stderr.trim_end() == "error: environment variable `DATABASE_URL` is not set",
        "the server started without DATABASE_URL, or said so unreadably ({}):\n{stderr}",
        output.status
    );
    Ok(())
}
