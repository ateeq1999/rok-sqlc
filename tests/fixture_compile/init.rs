//! A project made the way a user makes one: `rok-sqlc init`, a migration and
//! `rok-sqlc g`.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, ensure};

use crate::api_app::api_app_dir;
use crate::common::{self, cargo};

/// The project's migration: `users` and `posts`, and `tags`, which
/// [`LIST_ONLY`] leaves only its list.
const MIGRATION: &str = "CREATE TABLE users (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE posts (
    id UUID PRIMARY KEY,
    author_id UUID NOT NULL REFERENCES users (id),
    title TEXT NOT NULL CHECK (char_length(title) <= 120)
);
CREATE TABLE tags (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL
);
";

/// The `rok-sqlc.toml` lines skipping all of `tags`' endpoints but its list.
const LIST_ONLY: &str = "\n[api.skip]\ntags = [\"create\", \"get_one\", \"update\", \"remove\"]\n";

/// Runs `rok-sqlc <args>` in `dir`, which must succeed, and returns its
/// stderr.
fn rok_sqlc(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
        .args(args)
        .current_dir(dir)
        .output()
        .context("failed to run rok-sqlc")?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    ensure!(
        output.status.success(),
        "rok-sqlc {args:?} failed:\n{}{stderr}",
        String::from_utf8_lossy(&output.stdout),
    );
    Ok(stderr)
}

/// A project as after `cargo new <name>`, then `rok-sqlc init`, with
/// [`MIGRATION`] and `init`'s `rok-sqlc.toml` plus [`LIST_ONLY`].
fn scaffold(name: &str) -> Result<tempfile::TempDir> {
    let project = tempfile::TempDir::new()?;
    let dir = project.path();
    // A package already exists, as after `cargo new`, so `init` knows the
    // library crate `main.rs` calls into.
    std::fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
    )?;
    rok_sqlc(dir, &["init"])?;
    std::fs::write(dir.join("migrations/0001_init.sql"), MIGRATION)?;
    let config = dir.join("rok-sqlc.toml");
    let toml = std::fs::read_to_string(&config)?;
    std::fs::write(&config, format!("{toml}{LIST_ONLY}"))?;
    Ok(project)
}

/// Writes the project's `Cargo.toml`: exactly the dependencies the hints
/// name (`init`'s `rok-sqlc.toml` and the generated `src/db/mod.rs`), less
/// `left_out`, and the lints `rok-sqlc.toml` hints at; then the API
/// fixture's lock file, so builds resolve against it, not the newest
/// releases. Returns the manifest.
fn write_manifest(dir: &Path, name: &str, left_out: &[&str]) -> Result<String> {
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
        if let Some((dependency, _)) = hint.split_once(" = ")
            && !left_out.contains(&dependency)
        {
            dependencies.insert(dependency.to_owned(), hint.to_owned());
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
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\n{}\n\n[lints.clippy]\n{}\n",
        dependencies
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n"),
        lints.join("\n")
    );
    std::fs::write(dir.join("Cargo.toml"), &manifest)?;
    std::fs::copy(api_app_dir().join("Cargo.lock"), dir.join("Cargo.lock"))?;
    Ok(manifest)
}

/// Fails unless the project passes `cargo clippy --all-targets -- -D
/// warnings`, as its `AGENTS.md` requires: no warning about unused
/// generated code.
fn ensure_clippy_clean(dir: &Path, manifest: &str) -> Result<()> {
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

/// The project `rok-sqlc init` scaffolds, as a user gets it: `init`, a
/// migration, `rok-sqlc g` reading `init`'s `rok-sqlc.toml`, and a
/// `Cargo.toml` holding exactly the dependencies the two hint at and the
/// lints it hints at. It passes `cargo clippy --all-targets -- -D
/// warnings`, a list-only feature included, and serves its document.
#[test]
fn init_then_generate_builds_end_to_end() -> Result<()> {
    if let Some(reason) = common::generated_code_skip_reason(&api_app_dir()) {
        eprintln!("{reason}");
        return Ok(());
    }
    let project = scaffold("init-e2e")?;
    let dir = project.path();
    rok_sqlc(dir, &["g"])?;
    // What `init` and `g` made obeys the architecture.
    rok_sqlc(dir, &["check-arch"])?;
    let manifest = write_manifest(dir, "init-e2e", &[])?;
    // A test of the scaffolded `app.rs` serving the document and Swagger UI.
    std::fs::create_dir_all(dir.join("tests"))?;
    std::fs::write(dir.join("tests/docs.rs"), DOCS_TEST)?;

    ensure_clippy_clean(dir, &manifest)?;
    let output = cargo(dir, &["test", "--quiet", "--test", "docs"])
        .output()
        .context("failed to run cargo test")?;
    ensure!(
        output.status.success(),
        "the project `init` and `g` made doesn't serve its docs:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

/// Removes from the file at `path` every line whose text, less its
/// indentation, starts with one of `starts`, each of which must match.
fn remove_lines(path: &Path, starts: &[&str]) -> Result<()> {
    let text = std::fs::read_to_string(path)?;
    for start in starts {
        ensure!(
            text.lines()
                .any(|line| line.trim_start().starts_with(start)),
            "{} has no line starting `{start}`:\n{text}",
            path.display()
        );
    }
    let kept: String = text
        .lines()
        .filter(|line| {
            !starts
                .iter()
                .any(|start| line.trim_start().starts_with(start))
        })
        .map(|line| format!("{line}\n"))
        .collect();
    std::fs::write(path, kept)?;
    Ok(())
}

/// The same project with `[api] openapi = false`: `g` warns about the
/// scaffolded `src/openapi.rs`, and after the edits the OpenAPI guide lists
/// ("Turning the document off"), the project passes `cargo clippy
/// --all-targets -- -D warnings` without utoipa.
#[test]
fn init_with_openapi_off_builds_after_the_documented_edits() -> Result<()> {
    if let Some(reason) = common::generated_code_skip_reason(&api_app_dir()) {
        eprintln!("{reason}");
        return Ok(());
    }
    let project = scaffold("init-e2e-no-openapi")?;
    let dir = project.path();
    let config = dir.join("rok-sqlc.toml");
    let toml = std::fs::read_to_string(&config)?;
    let output = "output = \"src/features\"";
    ensure!(toml.contains(output), "{toml}");
    std::fs::write(
        &config,
        toml.replacen(output, &format!("{output}\nopenapi = false"), 1),
    )?;
    let warning = rok_sqlc(dir, &["g"])?;
    ensure!(
        warning.contains("warning: `[api] openapi = false`, but src/openapi.rs"),
        "{warning}"
    );

    // The guide's edits.
    std::fs::remove_file(dir.join("src/openapi.rs"))?;
    remove_lines(&dir.join("src/lib.rs"), &["pub mod openapi;"])?;
    remove_lines(
        &dir.join("src/app.rs"),
        &[
            "use utoipa_swagger_ui::SwaggerUi;",
            "use crate::openapi::document;",
            "cfg.service(web::redirect(\"/docs\", \"/docs/\")",
            ".service(SwaggerUi::new(",
        ],
    )?;
    let warning = rok_sqlc(dir, &["g"])?;
    ensure!(!warning.contains("warning"), "{warning}");
    rok_sqlc(dir, &["check-arch"])?;
    let manifest = write_manifest(dir, "init-e2e-no-openapi", &["utoipa-swagger-ui"])?;
    ensure!(!manifest.contains("utoipa"), "{manifest}");
    ensure_clippy_clean(dir, &manifest)
}

/// `tests/docs.rs` of the project: `/api-docs/openapi.json` answers the
/// document, with the generated paths, and `/docs/` Swagger UI. The pool
/// never connects: neither needs the database.
const DOCS_TEST: &str = r#"//! The document and Swagger UI the scaffolded `app.rs` serves.

use actix_web::{App, test, web};
use init_e2e::app::configure;
use init_e2e::shared::auth::JwtKeys;

#[actix_web::test]
async fn serves_the_document_and_swagger_ui() -> Result<(), Box<dyn std::error::Error>> {
    let pool = sqlx::PgPool::connect_lazy("postgres://localhost/unused")?;
    let keys = web::Data::new(JwtKeys::new("unused"));
    let app = test::init_service(App::new().configure(|cfg| configure(cfg, &pool, &keys))).await;
    let request = test::TestRequest::get().uri("/api-docs/openapi.json");
    let response = test::call_service(&app, request.to_request()).await;
    assert!(response.status().is_success());
    let body = test::read_body(response).await;
    let text = std::str::from_utf8(&body)?;
    assert!(text.contains("\"/api/v1/posts/{id}\""), "{text}");
    assert!(text.contains("\"bearer_auth\""), "{text}");
    let request = test::TestRequest::get().uri("/docs/");
    let response = test::call_service(&app, request.to_request()).await;
    assert!(response.status().is_success());
    Ok(())
}
"#;
