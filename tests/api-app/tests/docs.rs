//! The OpenAPI document (milestone 2): `app.rs` serves it at
//! `/api-docs/openapi.json` and Swagger UI at `/docs/`, and what it says of
//! each operation is what the handler does: its statuses, its security and
//! its params.

mod common;

use actix_web::http::{Method, StatusCode};
use common::{Database, call_as, document, fetch, token};
use rok_sqlc_api_app::shared::auth::Role;
use serde_json::{Value, json};
use uuid::Uuid;

/// The documented statuses of `method path`, sorted.
fn statuses(operation: &Value) -> Vec<u16> {
    let mut statuses: Vec<u16> = operation["responses"]
        .as_object()
        .expect("responses")
        .keys()
        .map(|status| status.parse().expect("a numeric status"))
        .collect();
    statuses.sort_unstable();
    statuses
}

/// Every `$ref` under `value`.
fn refs(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                match (key.as_str(), value) {
                    ("$ref", Value::String(target)) => found.push(target.clone()),
                    _ => refs(value, found),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| refs(item, found)),
        _ => {}
    }
}

#[actix_web::test]
async fn the_document_is_served_with_the_bearer_scheme() {
    let doc = document().await;
    assert_eq!(doc["openapi"], "3.1.0");
    assert_eq!(doc["info"]["title"], "rok-sqlc-api-app");
    assert_eq!(doc["info"]["version"], "0.1.0");
    assert_eq!(
        doc["components"]["securitySchemes"]["bearer_auth"],
        json!({
            "type": "http",
            "scheme": "bearer",
            "bearerFormat": "JWT",
            "description": "A JWT the application signed, sent as `Bearer <token>`.",
        })
    );
    let paths = doc["paths"].as_object().expect("paths");
    for path in [
        "/health/live",
        "/health/ready",
        "/api/v1/notes",
        "/api/v1/notes/{id}",
        "/api/v1/tasks/{id}",
        "/api/v1/audit-log",
    ] {
        assert!(paths.contains_key(path), "no {path}: {:?}", paths.keys());
    }
    let tags: Vec<&str> = doc["tags"]
        .as_array()
        .expect("tags")
        .iter()
        .filter_map(|tag| tag["name"].as_str())
        .collect();
    assert!(
        tags.contains(&"health") && tags.contains(&"audit-log"),
        "{tags:?}"
    );
}

#[actix_web::test]
async fn every_reference_resolves() {
    let doc = document().await;
    let mut found = Vec::new();
    refs(&doc, &mut found);
    assert!(!found.is_empty());
    for target in found {
        let name = target
            .strip_prefix("#/components/schemas/")
            .unwrap_or_else(|| panic!("{target} is not a schema"));
        assert!(
            doc["components"]["schemas"].get(name).is_some(),
            "{target} is not defined"
        );
    }
}

#[actix_web::test]
async fn swagger_ui_is_served_at_docs() {
    let (status, content_type, _, body) = fetch("/docs/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/html"), "{content_type}");
    assert!(String::from_utf8_lossy(&body).contains("swagger-ui"));
    let (status, _, location, _) = fetch("/docs").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location, "/docs/");
}

/// `GET /api/v1/tasks/{id}` as documented: a signed-in caller's (`401`
/// without one), its `id` a UUID (`400` otherwise), `404` for a row the
/// caller can't see, `200` with the row otherwise, and nothing else.
#[actix_web::test]
async fn a_documented_operation_matches_its_handler() {
    let doc = document().await;
    let operation = &doc["paths"]["/api/v1/tasks/{id}"]["get"];
    assert_eq!(operation["operationId"], "get_one");
    assert_eq!(operation["tags"], json!(["tasks"]));
    assert_eq!(operation["security"], json!([{"bearer_auth": []}]));
    let id = operation["parameters"]
        .as_array()
        .expect("parameters")
        .iter()
        .find(|param| param["name"] == "id")
        .expect("an `id` parameter");
    assert_eq!(id["in"], "path");
    assert_eq!(id["schema"], json!({"type": "string", "format": "uuid"}));
    for status in statuses(operation).iter().filter(|status| **status >= 400) {
        assert_eq!(
            operation["responses"][status.to_string()]["content"]["application/json"]["schema"],
            json!({"$ref": "#/components/schemas/ErrorBody"}),
            "{status}"
        );
    }
    // `403`: `?include=` of the owner, an `accounts` row, needs an admin.
    assert_eq!(statuses(operation), [200, 400, 401, 403, 404]);

    let Some(db) = Database::fresh().await else {
        return;
    };
    // The caller owns tasks as an account, which an admin creates.
    let admin = token(Uuid::now_v7(), Role::Admin);
    let account = call_as(
        &db,
        Some(&admin),
        Method::POST,
        "/api/v1/accounts",
        Some(json!({"email": "a@example.com", "password_hash": "x"})),
    )
    .await;
    assert_eq!(account.status, StatusCode::CREATED, "{account:?}");
    let caller = account.body["id"].as_str().expect("an id");
    let user = token(Uuid::parse_str(caller).expect("a UUID"), Role::User);
    let uri = |id: &str| format!("/api/v1/tasks/{id}");
    let missing = uri(&Uuid::now_v7().to_string());
    let answer = call_as(&db, None, Method::GET, &missing, None).await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{answer:?}");
    let answer = call_as(&db, Some(&user), Method::GET, &uri("not-a-uuid"), None).await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{answer:?}");
    let answer = call_as(&db, Some(&user), Method::GET, &missing, None).await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND, "{answer:?}");
    let created = call_as(
        &db,
        Some(&user),
        Method::POST,
        "/api/v1/tasks",
        Some(json!({"title": "documented"})),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{created:?}");
    let id = created.body["id"].as_str().expect("an id");
    let answer = call_as(&db, Some(&user), Method::GET, &uri(id), None).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    db.drop().await;
}

/// A sample value for a path parameter of `schema`.
fn sample(schema: &Value) -> String {
    match (schema["type"].as_str(), schema["format"].as_str()) {
        (Some("string"), Some("uuid")) => Uuid::now_v7().to_string(),
        (Some("integer"), _) => "1".to_owned(),
        _ => "sample".to_owned(),
    }
}

/// Every operation, called without a token: one carrying `bearer_auth`
/// answers `401`, and any other answers a status it documents.
#[actix_web::test]
async fn every_operation_needs_the_token_it_documents() {
    let doc = document().await;
    let Some(db) = Database::fresh().await else {
        return;
    };
    for (path, item) in doc["paths"].as_object().expect("paths") {
        for (method, operation) in item.as_object().expect("a path item") {
            let mut uri = path.clone();
            for param in operation["parameters"].as_array().into_iter().flatten() {
                if param["in"] == "path" {
                    let name = param["name"].as_str().expect("a name");
                    uri = uri.replace(&format!("{{{name}}}"), &sample(&param["schema"]));
                }
            }
            let method = Method::from_bytes(method.to_uppercase().as_bytes()).expect("a method");
            let body = operation.get("requestBody").map(|_| json!({}));
            let answer = call_as(&db, None, method.clone(), &uri, body).await;
            let required = operation["security"] == json!([{"bearer_auth": []}]);
            if required {
                assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{method} {uri}");
            } else {
                assert!(
                    statuses(operation).contains(&answer.status.as_u16()),
                    "{method} {uri} answered {}, documented {:?}",
                    answer.status,
                    statuses(operation)
                );
            }
        }
    }
    db.drop().await;
}
