//! `/api/v1/notes`: every endpoint of a table with a UUID key made in Rust,
//! its failures, and the list's paging, `q` and `sort`.

mod common;

use actix_web::http::{Method, StatusCode};
use common::{Database, assert_error, call, call_raw, create, get};
use serde_json::{Value, json};

const NOTES: &str = "/api/v1/notes";

/// A note with `slug`, `title` and `body`.
async fn note(db: &Database, slug: &str, title: &str, body: Option<&str>) -> Value {
    create(
        db,
        NOTES,
        json!({"slug": slug, "title": title, "body": body}),
    )
    .await
}

/// The `field` of each item of a list answer.
fn items(body: &Value, field: &str) -> Vec<Value> {
    body["items"]
        .as_array()
        .expect("an items array")
        .iter()
        .map(|item| item[field].clone())
        .collect()
}

#[actix_web::test]
async fn create_answers_201_with_the_row_and_its_location() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let answer = call(
        &db,
        Method::POST,
        NOTES,
        Some(json!({"slug": "first", "title": "First"})),
    )
    .await;
    assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
    let id = answer.body["id"].as_str().expect("a UUID id made in Rust");
    let id = uuid::Uuid::parse_str(id).expect("a UUID");
    assert_eq!(id.get_version_num(), 7);
    assert_eq!(answer.location, Some(format!("{NOTES}/{id}")));
    // Defaults apply; the response is the bare row.
    assert_eq!(answer.body["priority"], 0);
    assert_eq!(answer.body["body"], Value::Null);
    assert!(answer.body["created_at"].is_string(), "{answer:?}");
    let fields: Vec<&String> = answer.body.as_object().expect("an object").keys().collect();
    assert_eq!(
        fields,
        ["body", "created_at", "id", "priority", "slug", "title"]
    );

    let fetched = get(&db, &format!("{NOTES}/{id}")).await;
    assert_eq!(fetched.status, StatusCode::OK);
    assert_eq!(fetched.body, answer.body);
    db.drop().await;
}

#[actix_web::test]
async fn create_failures() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    note(&db, "taken", "Taken", None).await;

    // A unique violation is a conflict naming the constraint.
    let answer = call(
        &db,
        Method::POST,
        NOTES,
        Some(json!({"slug": "taken", "title": "Again"})),
    )
    .await;
    assert_error(&answer, StatusCode::CONFLICT, "conflict");
    let message = answer.body["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("notes_slug_key"), "{message}");

    // Broken rules are a validation failure, field by field.
    let answer = call(
        &db,
        Method::POST,
        NOTES,
        Some(json!({"slug": "long", "title": "x".repeat(81), "priority": 10})),
    )
    .await;
    assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY, "{answer:?}");
    assert_eq!(answer.code(), "validation_failed");
    let details = &answer.body["error"]["details"];
    assert!(details["title"].is_array(), "{answer:?}");
    assert!(details["priority"].is_array(), "{answer:?}");

    // Malformed JSON is a bad request; JSON that doesn't fit the body (a
    // missing field) fails validation.
    let answer = call_raw(&db, Method::POST, NOTES, "{\"slug\": ").await;
    assert_error(&answer, StatusCode::BAD_REQUEST, "bad_request");
    let answer = call(&db, Method::POST, NOTES, Some(json!({"slug": "x"}))).await;
    assert_error(
        &answer,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
    );
    db.drop().await;
}

#[actix_web::test]
async fn get_one_failures() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let missing = get(&db, &format!("{NOTES}/{}", uuid::Uuid::now_v7())).await;
    assert_error(&missing, StatusCode::NOT_FOUND, "not_found");
    let malformed = get(&db, &format!("{NOTES}/not-a-uuid")).await;
    assert_error(&malformed, StatusCode::BAD_REQUEST, "bad_request");
    db.drop().await;
}

#[actix_web::test]
async fn update_changes_only_the_fields_given() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let created = note(&db, "patch", "Before", Some("text")).await;
    let uri = format!("{NOTES}/{}", created["id"].as_str().unwrap_or_default());

    let answer = call(&db, Method::PATCH, &uri, Some(json!({"title": "After"}))).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.body["title"], "After");
    assert_eq!(answer.body["body"], "text");
    assert_eq!(answer.body["slug"], "patch");

    // `null` sets a nullable column to NULL; an empty patch changes nothing.
    let answer = call(&db, Method::PATCH, &uri, Some(json!({"body": null}))).await;
    assert_eq!(answer.body["body"], Value::Null, "{answer:?}");
    let answer = call(&db, Method::PATCH, &uri, Some(json!({}))).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.body["title"], "After");
    db.drop().await;
}

#[actix_web::test]
async fn update_failures() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let created = note(&db, "one", "One", None).await;
    note(&db, "two", "Two", None).await;
    let uri = format!("{NOTES}/{}", created["id"].as_str().unwrap_or_default());

    let missing = format!("{NOTES}/{}", uuid::Uuid::now_v7());
    let answer = call(&db, Method::PATCH, &missing, Some(json!({"title": "x"}))).await;
    assert_error(&answer, StatusCode::NOT_FOUND, "not_found");
    let answer = call(&db, Method::PATCH, &uri, Some(json!({"title": ""}))).await;
    assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY, "{answer:?}");
    assert_eq!(answer.code(), "validation_failed");
    let answer = call(&db, Method::PATCH, &uri, Some(json!({"slug": "two"}))).await;
    assert_error(&answer, StatusCode::CONFLICT, "conflict");
    let answer = call_raw(&db, Method::PATCH, &uri, "{\"title\"").await;
    assert_error(&answer, StatusCode::BAD_REQUEST, "bad_request");
    let answer = call(
        &db,
        Method::PATCH,
        &format!("{NOTES}/nope"),
        Some(json!({})),
    )
    .await;
    assert_error(&answer, StatusCode::BAD_REQUEST, "bad_request");
    db.drop().await;
}

#[actix_web::test]
async fn remove_answers_204_then_404() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let created = note(&db, "gone", "Gone", None).await;
    let uri = format!("{NOTES}/{}", created["id"].as_str().unwrap_or_default());

    let answer = call(&db, Method::DELETE, &uri, None).await;
    assert_eq!(answer.status, StatusCode::NO_CONTENT, "{answer:?}");
    assert_eq!(answer.body, Value::Null);
    let answer = call(&db, Method::DELETE, &uri, None).await;
    assert_error(&answer, StatusCode::NOT_FOUND, "not_found");
    assert_error(&get(&db, &uri).await, StatusCode::NOT_FOUND, "not_found");

    // A row another table references can't go: a conflict.
    let kept = note(&db, "kept", "Kept", None).await;
    let id = kept["id"].as_str().unwrap_or_default();
    create(
        &db,
        "/api/v1/members",
        json!({"team": "core", "note_id": id}),
    )
    .await;
    let answer = call(&db, Method::DELETE, &format!("{NOTES}/{id}"), None).await;
    assert_error(&answer, StatusCode::CONFLICT, "conflict");
    db.drop().await;
}

#[actix_web::test]
async fn list_pages_with_metadata() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let empty = get(&db, NOTES).await;
    assert_eq!(
        empty.body,
        json!({"items": [], "page": 1, "per_page": 20, "total": 0})
    );
    for index in 0..5 {
        note(&db, &format!("n{index}"), &format!("Note {index}"), None).await;
    }

    let answer = get(&db, &format!("{NOTES}?page=2&per_page=2")).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.body["page"], 2);
    assert_eq!(answer.body["per_page"], 2);
    assert_eq!(answer.body["total"], 5);
    // Newest first by default: `created_at` descending, ties by key.
    let all = get(&db, &format!("{NOTES}?per_page=100")).await;
    let slugs = items(&all.body, "slug");
    assert_eq!(slugs.len(), 5);
    assert_eq!(items(&answer.body, "slug"), slugs[2..4].to_vec());

    // `per_page` is capped at 100; past the last page is an empty page.
    let answer = get(&db, &format!("{NOTES}?per_page=1000&page=9")).await;
    assert_eq!(answer.body["per_page"], 100);
    assert_eq!(answer.body["items"], json!([]));
    assert_eq!(answer.body["total"], 5);

    // A malformed query is the wire standard's `400`.
    for query in ["page=abc", "sort=nope", "order=up", "sort=slug"] {
        let answer = get(&db, &format!("{NOTES}?{query}")).await;
        assert_error(&answer, StatusCode::BAD_REQUEST, "bad_request");
    }
    db.drop().await;
}

#[actix_web::test]
async fn list_sorts_by_the_configured_columns() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    // Titles sort the same under every collation (one case, no ties), since
    // `sort` follows the database's collation: byte order on a `C` or musl
    // locale, case-insensitive on glibc's `en_US.utf8`.
    for (slug, title, priority) in [("a", "cherry", 2), ("b", "date", 9), ("c", "banana", 5)] {
        create(
            &db,
            NOTES,
            json!({"slug": slug, "title": title, "priority": priority}),
        )
        .await;
    }
    let sorted = |query: &'static str| {
        let db = &db;
        async move { items(&get(db, &format!("{NOTES}?{query}")).await.body, "slug") }
    };
    assert_eq!(sorted("sort=priority&order=asc").await, ["a", "c", "b"]);
    assert_eq!(sorted("sort=priority").await, ["b", "c", "a"]);
    assert_eq!(sorted("sort=title&order=asc").await, ["c", "a", "b"]);
    assert_eq!(sorted("sort=created_at&order=asc").await, ["a", "b", "c"]);
    assert_eq!(sorted("").await, ["c", "b", "a"]);
    db.drop().await;
}

#[actix_web::test]
async fn list_q_matches_the_search_columns_literally_ignoring_case() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    note(&db, "alpha", "Alpha", None).await;
    note(&db, "under", "snake_case", None).await;
    note(&db, "pct", "100% done", Some("mentions ALPHA")).await;
    note(&db, "slash", "a\\b", None).await;
    let matched = |q: &'static str| {
        let db = &db;
        async move {
            let answer = get(db, &format!("{NOTES}?q={q}&sort=slug_is_not_sortable")).await;
            assert_eq!(answer.status, StatusCode::BAD_REQUEST);
            let answer = get(db, &format!("{NOTES}?q={q}&sort=title&order=asc")).await;
            assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
            let mut slugs = items(&answer.body, "slug");
            slugs.sort_by_key(ToString::to_string);
            (slugs, answer.body["total"].clone())
        }
    };
    // `title` or `body`, ignoring case.
    assert_eq!(matched("alp").await, (vec![json!("alpha"), json!("pct")], json!(2)));
    // `%`, `_` and `\` match themselves.
    assert_eq!(matched("%25").await, (vec![json!("pct")], json!(1)));
    assert_eq!(matched("_").await, (vec![json!("under")], json!(1)));
    assert_eq!(matched("%5C").await, (vec![json!("slash")], json!(1)));
    // `slug` is not a search column; an empty `q` matches every row.
    assert_eq!(matched("pct").await, (vec![], json!(0)));
    assert_eq!(matched("").await.1, json!(4));
    db.drop().await;
}

#[actix_web::test]
async fn unknown_routes_and_methods() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    // The join table `note_labels` has no feature.
    assert_eq!(
        get(&db, "/api/v1/note-labels").await.status,
        StatusCode::NOT_FOUND
    );
    // Outside the prefix.
    assert_eq!(get(&db, "/notes").await.status, StatusCode::NOT_FOUND);
    db.drop().await;
}
