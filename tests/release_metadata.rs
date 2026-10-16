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
