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

#[test]
fn ci_tests_the_declared_minimum_rust_version() -> Result<()> {
    let manifest: toml::Table = fs::read_to_string(root().join("Cargo.toml"))?.parse()?;
    let msrv = manifest["package"]["rust-version"]
        .as_str()
        .context("Cargo.toml must declare package.rust-version")?;
    let workflow = fs::read_to_string(Path::new(&root()).join(".github/workflows/ci.yml"))?;
    ensure!(
        workflow.contains(&format!("rust: [\"{msrv}\", stable]")),
        "the Linux CI matrix must test rust-version {msrv} and stable"
    );
    for fixture in ["tests/fixture-app", "tests/modules-app"] {
        let manifest: toml::Table =
            fs::read_to_string(root().join(fixture).join("Cargo.toml"))?.parse()?;
        ensure!(
            manifest["package"]
                .get("rust-version")
                .and_then(|v| v.as_str())
                == Some(msrv),
            "{fixture} must declare rust-version {msrv} so its lockfile stays buildable on the MSRV"
        );
    }
    Ok(())
}
