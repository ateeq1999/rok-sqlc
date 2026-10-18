//! A project made the way a user makes one: `rok-sqlc init`, a migration and
//! `rok-sqlc g`.

use std::process::Command;

use anyhow::{Context, Result, ensure};

use crate::api_app::api_app_dir;
use crate::common::{self, cargo};

/// The project `rok-sqlc init` scaffolds, as a user gets it: `init`, a
/// migration, `rok-sqlc g` reading `init`'s `rok-sqlc.toml`, and a
/// `Cargo.toml` holding exactly the dependencies the two hint at (`init`'s
/// `rok-sqlc.toml` and the generated `src/db/mod.rs`) and the lints it
/// hints at. It passes `cargo clippy --all-targets -- -D warnings`, as its
/// `AGENTS.md` requires: no warning about unused generated code.
#[test]
fn init_then_generate_builds_end_to_end() -> Result<()> {
    if let Some(reason) = common::generated_code_skip_reason(&api_app_dir()) {
        eprintln!("{reason}");
        return Ok(());
    }
    let project = tempfile::TempDir::new()?;
    let dir = project.path();
    let rok_sqlc = |args: &[&str]| -> Result<()> {
        let output = Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
            .args(args)
            .current_dir(dir)
            .output()
            .context("failed to run rok-sqlc")?;
        ensure!(
            output.status.success(),
            "rok-sqlc {args:?} failed:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    };
    // A package already exists, as after `cargo new`, so `init` knows the
    // library crate `main.rs` calls into.
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"init-e2e\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    rok_sqlc(&["init"])?;
    std::fs::write(
        dir.join("migrations/0001_init.sql"),
        "CREATE TABLE users (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE posts (
    id UUID PRIMARY KEY,
    author_id UUID NOT NULL REFERENCES users (id),
    title TEXT NOT NULL CHECK (char_length(title) <= 120)
);
",
    )?;
    rok_sqlc(&["g"])?;
    // What `init` and `g` made obeys the architecture.
    rok_sqlc(&["check-arch"])?;

    // The dependency lines of `rok-sqlc.toml`'s `# [dependencies]` block,
    // then the data layer's `//   name = ...` lines, which win a tie.
    let mut dependencies = std::collections::BTreeMap::new();
    let toml = std::fs::read_to_string(dir.join("rok-sqlc.toml"))?;
    let block = toml
        .lines()
        .skip_while(|line| *line != "# [dependencies]")
        .skip(1)
        .take_while(|line| *line != "#")
        .filter_map(|line| line.strip_prefix("# "));
    let db_mod = std::fs::read_to_string(dir.join("src/db/mod.rs"))?;
    let generated = db_mod.lines().filter_map(|line| line.strip_prefix("//   "));
    for hint in block.chain(generated) {
        if let Some((name, _)) = hint.split_once(" = ") {
            dependencies.insert(name.to_owned(), hint.to_owned());
        }
    }
    ensure!(
        dependencies.contains_key("actix-web") && dependencies.contains_key("sqlx"),
        "the hints name no dependencies: {dependencies:?}"
    );
    // The `# [lints.clippy]` block after it.
    let lints = toml
        .lines()
        .skip_while(|line| *line != "# [lints.clippy]")
        .skip(1)
        .take_while(|line| *line != "#")
        .filter_map(|line| line.strip_prefix("# "))
        .collect::<Vec<_>>();
    ensure!(!lints.is_empty(), "the hints name no lints");
    let manifest = format!(
        "[package]\nname = \"init-e2e\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\n{}\n\n[lints.clippy]\n{}\n",
        dependencies
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n"),
        lints.join("\n")
    );
    std::fs::write(dir.join("Cargo.toml"), &manifest)?;
    // Resolve against the API fixture's lock file, not the newest releases.
    std::fs::copy(api_app_dir().join("Cargo.lock"), dir.join("Cargo.lock"))?;

    let output = cargo(
        dir,
        &["clippy", "--quiet", "--all-targets", "--", "-D", "warnings"],
    )
    .output()
    .context("failed to run cargo clippy")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    ensure!(
        output.status.success() && !stderr.contains("warning"),
        "the project `init` and `g` made fails `cargo clippy --all-targets -- -D warnings`:\n{manifest}\n{stderr}"
    );
    Ok(())
}
