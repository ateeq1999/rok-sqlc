//! Helpers shared by the integration suites. Each suite uses only some of
//! them.
#![allow(dead_code)]

use std::path::Path;
use std::process::Command;

/// A path the way rok-sqlc prints one outside the current directory: `/`
/// as the separator on every platform.
pub fn shown(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// Generated code targets SQLx 0.9, whose minimum Rust version is above the
/// tool's own. Returns why a suite that builds a fixture crate must skip on
/// the current toolchain, or `None` when the toolchain meets the floor the
/// fixture crate declares in its `rust-version`.
pub fn generated_code_skip_reason(fixture: &Path) -> Option<String> {
    let manifest: toml::Table = std::fs::read_to_string(fixture.join("Cargo.toml"))
        .expect("read the fixture crate's Cargo.toml")
        .parse()
        .expect("parse the fixture crate's Cargo.toml");
    let floor = manifest["package"]["rust-version"]
        .as_str()
        .expect("the fixture crate declares package.rust-version")
        .to_owned();
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let output = Command::new(rustc)
        .arg("--version")
        .output()
        .expect("run rustc --version");
    let version = String::from_utf8_lossy(&output.stdout);
    // "rustc 1.94.0 (...)"
    let current = version
        .split_whitespace()
        .nth(1)
        .expect("rustc --version prints a version")
        .to_owned();
    (parse(&current) < parse(&floor)).then(|| {
        format!(
            "skipping: generated code needs Rust {floor} (SQLx 0.9); this toolchain is {current}"
        )
    })
}

/// `1.94` or `1.94.0-nightly` as `(major, minor, patch)`.
fn parse(version: &str) -> (u32, u32, u32) {
    let mut parts = version
        .split(['.', '-'])
        .map(|part| part.parse().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}
