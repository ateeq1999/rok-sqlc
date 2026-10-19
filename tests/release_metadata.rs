//! Release metadata: what `cargo package` ships, and that the declared
//! minimum supported Rust version is the one CI tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, ensure};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn packaged_files() -> Result<Vec<String>> {
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["package", "--list", "--allow-dirty", "--offline"])
        .current_dir(root())
        .output()
        .context("failed to run cargo package --list")?;
    ensure!(
        output.status.success(),
        "cargo package --list failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .map(|line| line.replace('\\', "/"))
        .collect())
}

#[test]
fn package_ships_licenses_readme_and_changelog_but_no_fixtures() -> Result<()> {
    let files = packaged_files()?;
    for required in [
        "Cargo.toml",
        "README.md",
        "CHANGELOG.md",
        "LICENSE-MIT",
        "LICENSE-APACHE",
        "src/lib.rs",
        "src/main.rs",
        // `init` embeds its templates with `include_str!`.
        "src/init/templates/AGENTS.md",
        "src/init/templates/rok-sqlc.toml",
        // `init --template` embeds `templates/` through `build.rs`.
        "build.rs",
        "templates/actix-api/rok-sqlc.toml",
        "templates/actix-api/migrations/0001_init.sql",
    ] {
        ensure!(
            files.iter().any(|file| file == required),
            "{required} missing from the package: {files:?}"
        );
    }
    for excluded in [".scratch/", "modules/", "tests/", "docs/", ".github/"] {
        ensure!(
            !files.iter().any(|file| file.starts_with(excluded)),
            "{excluded} must not be packaged: {files:?}"
        );
    }
    // A template's build output, when the template was built in place.
    ensure!(
        !files
            .iter()
            .any(|file| file.starts_with("templates/") && file.split('/').nth(2) == Some("target")),
        "a template's target/ must not be packaged: {files:?}"
    );
    Ok(())
}

/// Every file of every template is packaged, so `init --template` from
/// `cargo install` copies what it does from the repository. (Cargo leaves
/// out a directory holding a `Cargo.toml`: a template's manifest is
/// `Cargo.toml.template`.)
#[test]
fn package_ships_every_template_file() -> Result<()> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let relative = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                // A template's build in place.
                if relative.split('/').nth(2) == Some("target") {
                    continue;
                }
                walk(&path, root, out)?;
            } else {
                out.push(relative);
            }
        }
        Ok(())
    }
    let mut templates = Vec::new();
    walk(&root().join("templates"), &root(), &mut templates)?;
    ensure!(!templates.is_empty(), "templates/ holds no files");
    let files = packaged_files()?;
    let missing: Vec<_> = templates
        .iter()
        .filter(|file| !files.contains(file))
        .collect();
    ensure!(
        missing.is_empty(),
        "template files not packaged: {missing:?}"
    );
    Ok(())
}

fn rust_version(manifest_dir: &Path) -> Result<String> {
    let manifest: toml::Table = fs::read_to_string(manifest_dir.join("Cargo.toml"))?.parse()?;
    manifest["package"]
        .get("rust-version")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .with_context(|| {
            format!(
                "{} must declare package.rust-version",
                manifest_dir.display()
            )
        })
}

/// Two floors: the tool's own `rust-version`, and the higher one generated
/// code needs (SQLx 0.9's), which the fixture crates declare. The Linux CI
/// matrix tests both, plus stable.
#[test]
fn ci_tests_both_declared_minimum_rust_versions() -> Result<()> {
    let msrv = rust_version(&root())?;
    let generated_floor = rust_version(&root().join("tests/fixture-app"))?;
    ensure!(
        rust_version(&root().join("tests/modules-app"))? == generated_floor,
        "tests/modules-app must declare the same rust-version as tests/fixture-app ({generated_floor})"
    );
    ensure!(
        rust_version(&root().join("tests/api-app"))? == generated_floor,
        "tests/api-app must declare the same rust-version as tests/fixture-app ({generated_floor})"
    );
    ensure!(
        generated_floor == "1.94",
        "generated code targets SQLx 0.9, whose MSRV is 1.94; the fixture crates declare {generated_floor}"
    );
    let workflow = fs::read_to_string(Path::new(&root()).join(".github/workflows/ci.yml"))?;
    ensure!(
        workflow.contains(&format!(
            "rust: [\"{msrv}\", \"{generated_floor}\", stable]"
        )),
        "the Linux CI matrix must test the tool's rust-version {msrv}, \
         generated code's floor {generated_floor}, and stable"
    );
    Ok(())
}
