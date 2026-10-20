//! API features for axum (`[api] framework = "axum"`, 0.7 decisions A2 and
//! A3) through the library entry point: the framework changes only the
//! HTTP edge, so every other file is byte-identical to actix-web's, and the
//! edge is checked against the expected files in `tests/api_axum/`. That
//! the code builds and serves is tested by compiling a project
//! (`tests/fixture_compile/init_axum.rs`).
//!
//! After a deliberate change to the axum output, rewrite the expected files
//! with `ROK_SQLC_UPDATE_EXPECTED=1 cargo test --test api_axum` and review
//! the diff.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use rok_sqlc::{ApiAccess, ApiConfig, ApiFramework, Config, Derive, Extractor};
use tempfile::TempDir;

/// `accounts`: admin only, versioned, with a unique `email` (a `by-`
/// route), having many `notes`. `notes`: public, soft-deleted, embedding
/// its account (`?include=account`, whose admin access the handler checks
/// itself), with many `tags` through `note_tags`. `tasks`: owned and
/// tenant-scoped. `memberships`: a composite text key.
const SCHEMA: &str = "CREATE TABLE accounts (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    revision INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE notes (
    id BIGSERIAL PRIMARY KEY,
    account_id UUID NOT NULL REFERENCES accounts (id),
    title TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);
CREATE TABLE tags (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE note_tags (
    note_id BIGINT NOT NULL REFERENCES notes (id),
    tag_id BIGINT NOT NULL REFERENCES tags (id),
    PRIMARY KEY (note_id, tag_id)
);
CREATE TABLE tasks (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    owner_id UUID NOT NULL,
    title TEXT NOT NULL
);
CREATE TABLE memberships (
    org TEXT NOT NULL,
    email TEXT NOT NULL,
    role TEXT,
    PRIMARY KEY (org, email)
);
";

/// The files the framework changes, under `src/`.
const EDGE: &[&str] = &[
    "features/generated.rs",
    "features/README.md",
    "shared/health.rs",
    "shared/tenant.rs",
    "shared/validation.rs",
];

/// [`SCHEMA`] generated for `framework` into a fresh directory, with both
/// extractors, so the data layer's `http/` is the same.
fn generate(framework: ApiFramework) -> Result<TempDir> {
    let dir = TempDir::new()?;
    let path = |path: &str| dir.path().join(path);
    fs::write(path("schema.sql"), SCHEMA)?;
    let mut config = Config::default();
    config.schema = vec![path("schema.sql")];
    config.output = path("src/db");
    config.derive = vec![Derive::Serde, Derive::Validate];
    config.extractors = vec![Extractor::ActixWeb, Extractor::Axum];
    config.conventions.soft_delete_column = Some("deleted_at".into());
    config.conventions.version_column = Some("revision".into());
    config.conventions.tenant_column = Some("tenant_id".into());
    let mut api = ApiConfig::new(path("src/features"));
    api.framework = framework;
    api.search_endpoint = true;
    api.access.insert("accounts".into(), ApiAccess::Admin);
    api.access.insert("notes".into(), ApiAccess::Public);
    api.owner.insert("tasks".into(), "owner_id".into());
    config.api = Some(api);
    rok_sqlc::generate(&config)?;
    Ok(dir)
}

/// Every file under `dir`, by its path relative to it with `/`.
fn read_tree(dir: &Path) -> Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path.strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
                files.insert(relative, fs::read_to_string(&path)?);
            }
        }
    }
    Ok(files)
}

/// Whether the framework changes the file at `path`: the edge, each
/// feature's `routes.rs` and `handlers.rs`, and the data layer's `mod.rs`
/// (its Swagger UI hint).
fn is_edge(path: &str) -> bool {
    EDGE.contains(&path)
        || path.ends_with("/routes.rs")
        || path.ends_with("/handlers.rs")
        || path == "db/mod.rs"
}

#[test]
fn only_the_http_edge_differs_from_actix() -> Result<()> {
    let actix = read_tree(&generate(ApiFramework::Actix)?.path().join("src"))?;
    let axum = read_tree(&generate(ApiFramework::Axum)?.path().join("src"))?;

    assert_eq!(
        actix.keys().collect::<Vec<_>>(),
        axum.keys().collect::<Vec<_>>()
    );
    let mut same = 0;
    for (path, text) in &actix {
        if is_edge(path) {
            assert_ne!(text, &axum[path], "{path} is the same for axum");
        } else {
            assert_eq!(text, &axum[path], "{path} differs for axum");
            same += 1;
        }
    }
    // Each feature's dto.rs, model.rs, repo.rs, service_api.rs and
    // service.rs, among others, are the same.
    for feature in ["accounts", "memberships", "notes", "tags", "tasks"] {
        for file in [
            "dto.rs",
            "model.rs",
            "repo.rs",
            "service_api.rs",
            "service.rs",
        ] {
            let path = format!("features/{feature}/{file}");
            assert_eq!(actix.get(&path), axum.get(&path), "{path}");
        }
    }
    assert!(same > 40, "{same}");
    // The data layer's `mod.rs` differs only in Swagger UI's feature.
    assert_eq!(
        actix["db/mod.rs"].replace("[\"actix-web\", \"vendored\"]", "[\"axum\", \"vendored\"]"),
        axum["db/mod.rs"]
    );
    Ok(())
}

/// The expected files: `tests/api_axum/<path>`.
fn expected_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/api_axum")
}

#[test]
fn the_axum_edge_matches_the_expected_files() -> Result<()> {
    let dir = generate(ApiFramework::Axum)?;
    let files = read_tree(&dir.path().join("src"))?;
    let edge: Vec<(&String, &String)> = files
        .iter()
        .filter(|(path, _)| is_edge(path) && *path != "db/mod.rs")
        .collect();
    let update = std::env::var_os("ROK_SQLC_UPDATE_EXPECTED").is_some();
    let mut differing = Vec::new();
    for (path, text) in &edge {
        let expected = expected_dir().join(path);
        if update {
            fs::create_dir_all(expected.parent().expect("a parent"))?;
            fs::write(&expected, text)?;
            continue;
        }
        let want = fs::read_to_string(&expected)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        if want != **text {
            differing.push((*path).clone());
        }
    }
    assert!(
        differing.is_empty(),
        "the axum output differs from tests/api_axum/ in {differing:?}; \
         if the change is deliberate, rewrite them with ROK_SQLC_UPDATE_EXPECTED=1"
    );
    // No expected file is left over.
    let expected = read_tree(&expected_dir())?;
    let generated: Vec<&String> = edge.iter().map(|(path, _)| *path).collect();
    for path in expected.keys() {
        assert!(generated.contains(&path), "tests/api_axum/{path} is stale");
    }
    Ok(())
}

/// The generated axum code is laid out as rustfmt lays it out, and names
/// no actix-web.
#[test]
fn the_axum_output_is_rustfmt_clean_and_actix_free() -> Result<()> {
    let dir = generate(ApiFramework::Axum)?;
    let src = dir.path().join("src");
    let files: Vec<PathBuf> = read_tree(&src)?
        .into_iter()
        .filter(|(path, _)| path.ends_with(".rs"))
        .inspect(|(path, text)| {
            if !path.starts_with("db/") {
                assert!(!text.contains("actix"), "{path} names actix");
            }
        })
        .map(|(path, _)| src.join(path))
        .collect();
    let output = std::process::Command::new("rustfmt")
        .args(["--edition", "2024", "--check"])
        .args(&files)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    Ok(())
}
