//! The OpenAPI document as a whole (milestone 2): the served
//! `/api-docs/openapi.json` matches the checked-in snapshot,
//! `tests/snapshots/openapi.json`, and every operation in it follows the
//! wire standard.
//!
//! A change to the generated annotations fails the snapshot test. When the
//! change is meant, rewrite the snapshot and review its diff:
//!
//! ```text
//! ROK_SQLC_UPDATE_SNAPSHOTS=1 cargo test --test openapi
//! ```
//!
//! (`tests/fixture_compile/` in rok-sqlc generates this crate first.)

mod common;

use std::path::PathBuf;

use common::document;
use serde_json::{Map, Value};

/// Set to rewrite the snapshot from the served document.
const UPDATE: &str = "ROK_SQLC_UPDATE_SNAPSHOTS";

/// `[api] prefix`, the default.
const PREFIX: &str = "/api/v1/";

/// The snapshot file.
fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/openapi.json")
}

/// `value` with every object's keys sorted, so the text is stable whatever
/// order utoipa writes them in.
fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut out = Map::new();
            for key in keys {
                out.insert(key.clone(), sorted(&map[key]));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// The document's text as the snapshot holds it: sorted keys, two-space
/// indents, a final newline.
fn snapshot_text(doc: &Value) -> String {
    let mut text = serde_json::to_string_pretty(&sorted(doc)).expect("a JSON document");
    text.push('\n');
    text
}

#[actix_web::test]
async fn the_document_matches_the_snapshot() {
    let actual = snapshot_text(&document().await);
    let path = snapshot_path();
    if std::env::var_os(UPDATE).is_some() {
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("create snapshots/");
        std::fs::write(&path, &actual).expect("write the snapshot");
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| {
            panic!(
                "no snapshot at {}; set {UPDATE}=1 to write it",
                path.display()
            )
        })
        .replace("\r\n", "\n");
    if actual != expected {
        let line = actual
            .lines()
            .zip(expected.lines())
            .position(|(a, e)| a != e)
            .unwrap_or_else(|| actual.lines().count().min(expected.lines().count()));
        let around = |text: &str| -> String {
            text.lines()
                .skip(line.saturating_sub(3))
                .take(7)
                .collect::<Vec<_>>()
                .join("\n")
        };
        panic!(
            "the served document differs from {} at line {}:\n--- snapshot\n{}\n--- served\n{}\nIf the change is meant, rerun with {UPDATE}=1 and review the diff.",
            path.display(),
            line + 1,
            around(&expected),
            around(&actual),
        );
    }
}

/// Every operation: its path, method and operation object.
fn operations(doc: &Value) -> Vec<(String, String, Value)> {
    let mut found = Vec::new();
    for (path, item) in doc["paths"].as_object().expect("paths") {
        for (method, operation) in item.as_object().expect("a path item") {
            found.push((path.clone(), method.clone(), operation.clone()));
        }
    }
    assert!(found.len() > 20, "{found:?}");
    found
}

/// The schema `$ref` of `operation`'s `status` response, if any.
fn response_ref(operation: &Value, status: &str) -> Option<String> {
    operation["responses"][status]["content"]["application/json"]["schema"]["$ref"]
        .as_str()
        .map(str::to_owned)
}

/// Whether `path` needs a caller, from the fixture's access per feature
/// (`tests/fixture_compile/api_app.rs`): `accounts` and `documents` are
/// admin only, `tasks` keeps the default (`user`), and the rest are public.
/// A relationship route needs what the stricter of its two features needs.
fn needs_caller(path: &str) -> bool {
    path.split('/')
        .any(|segment| matches!(segment, "accounts" | "documents" | "tasks"))
}

#[actix_web::test]
async fn every_path_is_under_the_prefix_but_health() {
    let doc = document().await;
    for (path, method, operation) in operations(&doc) {
        if operation["tags"] == serde_json::json!(["health"]) {
            assert!(path.starts_with("/health/"), "{method} {path}");
        } else {
            assert!(path.starts_with(PREFIX), "{method} {path}");
        }
    }
}

#[actix_web::test]
async fn every_error_response_is_an_error_body() {
    let doc = document().await;
    for (path, method, operation) in operations(&doc) {
        let responses = operation["responses"].as_object().expect("responses");
        for status in responses.keys().filter(|status| !status.starts_with('2')) {
            assert_eq!(
                response_ref(&operation, status).as_deref(),
                Some("#/components/schemas/ErrorBody"),
                "{method} {path} {status}"
            );
        }
    }
}

/// Lists answer a page, creates `201` and deletes `204`.
#[actix_web::test]
async fn lists_creates_and_deletes_answer_the_standard_statuses() {
    let doc = document().await;
    let (mut lists, mut creates, mut deletes) = (0, 0, 0);
    for (path, method, operation) in operations(&doc) {
        let id = operation["operationId"].as_str().expect("an operation id");
        // A feature's list and search; a relationship's `list_<name>` answers
        // every related row.
        if id == "list" || id == "search" {
            lists += 1;
            let page = response_ref(&operation, "200").unwrap_or_default();
            assert!(
                page.starts_with("#/components/schemas/PaginatedResponse_"),
                "{method} {path}: {page}"
            );
        }
        if method == "post" && id == "create" {
            creates += 1;
            assert!(
                operation["responses"].get("201").is_some(),
                "{method} {path}"
            );
            assert!(
                operation["responses"].get("200").is_none(),
                "{method} {path}"
            );
        }
        if method == "delete" {
            deletes += 1;
            assert!(
                operation["responses"].get("204").is_some(),
                "{method} {path}"
            );
            assert!(
                operation["responses"].get("200").is_none(),
                "{method} {path}"
            );
        }
    }
    assert!(
        lists > 0 && creates > 0 && deletes > 0,
        "{lists} {creates} {deletes}"
    );
}

/// A route needing a caller carries `bearer_auth`; a public one doesn't,
/// unless it offers it as optional (`[{}, {"bearer_auth": []}]`: a stricter
/// `?include=`).
#[actix_web::test]
async fn routes_needing_a_caller_carry_bearer_auth() {
    let doc = document().await;
    let required = serde_json::json!([{"bearer_auth": []}]);
    let optional = serde_json::json!([{}, {"bearer_auth": []}]);
    for (path, method, operation) in operations(&doc) {
        let security = &operation["security"];
        if needs_caller(&path) {
            assert_eq!(security, &required, "{method} {path}");
        } else {
            assert!(
                security.is_null() || security == &optional,
                "{method} {path}: {security}"
            );
        }
    }
}

/// The generated handlers' sources: every feature's `handlers.rs`, and
/// health's.
fn handler_sources() -> Vec<String> {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources =
        vec![std::fs::read_to_string(src.join("shared/health.rs")).expect("health.rs")];
    for entry in std::fs::read_dir(src.join("features")).expect("src/features") {
        let handlers = entry.expect("an entry").path().join("handlers.rs");
        if let Ok(source) = std::fs::read_to_string(handlers) {
            sources.push(source);
        }
    }
    assert!(sources.len() > 5, "{} handler files", sources.len());
    sources
}

/// Operation ids are handler names (`snake_case`), and tags are lowercase
/// kebab-case, one per operation.
#[actix_web::test]
async fn operation_ids_are_handler_names_and_tags_are_kebab_case() {
    let doc = document().await;
    let snake = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    let kebab = |text: &str| {
        !text.is_empty()
            && !text.starts_with('-')
            && text
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    let handlers = handler_sources();
    for (path, method, operation) in operations(&doc) {
        let id = operation["operationId"].as_str().expect("an operation id");
        assert!(snake(id), "{method} {path}: {id}");
        let handler = format!("pub async fn {id}(");
        assert!(
            handlers.iter().any(|source| source.contains(&handler)),
            "{method} {path}: no handler `{id}`"
        );
        let tags = operation["tags"].as_array().expect("tags");
        assert_eq!(tags.len(), 1, "{method} {path}");
        let tag = tags[0].as_str().expect("a tag");
        assert!(kebab(tag), "{method} {path}: {tag}");
        let declared = doc["tags"]
            .as_array()
            .expect("tags")
            .iter()
            .any(|declared| declared["name"] == tag);
        assert!(declared, "{tag} is not declared");
    }
    // Health's are the target document's.
    assert_eq!(
        doc["paths"]["/health/live"]["get"]["operationId"],
        "liveness"
    );
    assert_eq!(
        doc["paths"]["/health/ready"]["get"]["operationId"],
        "readiness"
    );
}
