//! What the template tests share: a template of `templates/`, made the way
//! a user makes it (`rok-sqlc init --template <name>` into a scratch
//! directory, then `rok-sqlc g` there), checked against what it checks in,
//! its README against its manifest, and the project's own checks and tests.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, ensure};

use crate::common::{self, cargo};
use crate::rok_sqlc;

/// A template of `templates/`.
#[derive(Clone, Copy)]
pub struct Template {
    /// The name `init --template` takes.
    pub name: &'static str,
    /// Its directory under `templates/`.
    pub dir: &'static str,
}

impl Template {
    /// The template's directory.
    fn path(self) -> PathBuf {
        common::manifest_dir().join("templates").join(self.dir)
    }

    /// A project made from the template: `init --template` (which copies
    /// the template's `Cargo.lock`, so builds resolve as the template's
    /// do), then `g`.
    fn materialize(self) -> Result<tempfile::TempDir> {
        let project = tempfile::TempDir::new()?;
        let dir = project.path();
        rok_sqlc(dir, &["init", "--template", self.name])?;
        rok_sqlc(dir, &["g"])?;
        Ok(project)
    }

    /// The template checks in exactly the files `rok-sqlc g` generates from
    /// its schema and `rok-sqlc.toml`. With `ROK_SQLC_UPDATE_EXPECTED` set,
    /// it rewrites them instead.
    pub fn checks_in_what_generation_writes(self) -> Result<()> {
        let project = self.materialize()?;
        let generated = generated_files(project.path())?;
        ensure!(
            generated.contains_key(Path::new("src/features/users/dto.rs")),
            "`rok-sqlc g` generated no users feature: {:?}",
            generated.keys()
        );
        let template = self.path();
        let checked_in = generated_files(&template)?;
        if std::env::var_os("ROK_SQLC_UPDATE_EXPECTED").is_some() {
            for path in checked_in
                .keys()
                .filter(|path| !generated.contains_key(*path))
            {
                std::fs::remove_file(template.join(path))?;
            }
            for (path, text) in &generated {
                let target = template.join(path);
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
            .collect::<BTreeSet<_>>()
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
            "templates/{} drifted from its generation (regenerate with ROK_SQLC_UPDATE_EXPECTED=1):\n  {}",
            self.dir,
            drift.join("\n  ")
        );
        Ok(())
    }

    /// The template's README lists the dependencies and lints of its
    /// `Cargo.toml`, for a project that keeps its own manifest.
    pub fn readme_lists_its_manifest(self) -> Result<()> {
        let template = self.path();
        let manifest = std::fs::read_to_string(template.join("Cargo.toml.template"))?;
        let readme = std::fs::read_to_string(template.join("README.md"))?.replace("\r\n", "\n");
        let listed: String = manifest
            .replace("\r\n", "\n")
            .lines()
            .skip_while(|line| *line != "[dependencies]")
            .filter(|line| !line.starts_with('#'))
            .map(|line| format!("{line}\n"))
            .collect();
        ensure!(
            readme.contains(&format!("```toml\n{}\n```", listed.trim_end())),
            "the README.md dependency block of templates/{} differs from Cargo.toml.template's:\n{listed}",
            self.dir
        );
        Ok(())
    }

    /// The project made from the template is rustfmt-clean, passes `cargo
    /// clippy --all-targets -- -D warnings` and `check-arch`, and passes
    /// its tests: the contract, and the API tests against PostgreSQL when
    /// `ROK_SQLC_TEST_DATABASE_URL` is set (they skip otherwise); and
    /// started unconfigured, it fails readably.
    pub fn passes_its_checks_and_tests(self) -> Result<()> {
        let project = self.materialize()?;
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
            "the {} template is not rustfmt-clean:\n{}",
            self.name,
            String::from_utf8_lossy(&output.stdout)
        );
        self.cargo_clean(
            dir,
            &["clippy", "--quiet", "--all-targets", "--", "-D", "warnings"],
            None,
        )?;
        let database = std::env::var("ROK_SQLC_TEST_DATABASE_URL").ok();
        self.cargo_clean(dir, &["test", "--quiet"], database.as_deref())?;
        starts_only_when_configured(dir)
    }

    /// `cargo <args>` in `dir`, which must succeed without a warning.
    fn cargo_clean(self, dir: &Path, args: &[&str], database: Option<&str>) -> Result<()> {
        let mut command = cargo(dir, args);
        match database {
            Some(url) => command.env("DATABASE_URL", url),
            None => command.env_remove("DATABASE_URL"),
        };
        let output = command.output().context("failed to run cargo")?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        ensure!(
            output.status.success() && !stderr.contains("warning:"),
            "`cargo {}` fails in the {} template:\n{}{stderr}",
            args.join(" "),
            self.name,
            String::from_utf8_lossy(&output.stdout)
        );
        Ok(())
    }
}

/// rok-sqlc's generated files under `root` (a build's `target/` aside), by
/// relative path, with `\n` line endings.
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
