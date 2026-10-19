//! The other shapes of API feature: a composite key (`members`), no key
//! (`audit_events`, under the configured route base `/audit-log`), a key
//! the database makes with an endpoint skipped (`labels`), and a scaffolded
//! service the user changed (`drafts`).

mod common;

use actix_web::http::{Method, StatusCode};
use common::{Database, assert_error, call, create, get};
use serde_json::{Value, json};

const MEMBERS: &str = "/api/v1/members";

async fn note_id(db: &Database, slug: &str) -> String {
    let note = create(db, "/api/v1/notes", json!({"slug": slug, "title": slug})).await;
    note["id"].as_str().unwrap_or_default().to_owned()
}

#[actix_web::test]
async fn composite_keys_go_in_the_path_in_order() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let note = note_id(&db, "shared").await;
    let answer = call(
        &db,
        Method::POST,
        MEMBERS,
        Some(json!({"team": "red team/1", "note_id": note})),
    )
    .await;
    assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
    assert_eq!(answer.body["role"], "viewer");
    // Each key segment is percent-encoded.
    let uri = format!("{MEMBERS}/red%20team%2F1/{note}");
    assert_eq!(answer.location.as_deref(), Some(uri.as_str()));

    let fetched = get(&db, &uri).await;
    assert_eq!(fetched.status, StatusCode::OK, "{fetched:?}");
    assert_eq!(fetched.body, answer.body);

    let answer = call(&db, Method::PATCH, &uri, Some(json!({"role": "editor"}))).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.body["role"], "editor");
    assert_eq!(answer.body["team"], "red team/1");

    assert_eq!(
        call(&db, Method::DELETE, &uri, None).await.status,
        StatusCode::NO_CONTENT
    );
    assert_error(&get(&db, &uri).await, StatusCode::NOT_FOUND, "not_found");

    // A foreign key to no row is a conflict naming the constraint.
    let answer = call(
        &db,
        Method::POST,
        MEMBERS,
        Some(json!({"team": "x", "note_id": uuid::Uuid::now_v7()})),
    )
    .await;
    assert_error(&answer, StatusCode::CONFLICT, "conflict");
    let message = answer.body["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("members_note_id_fkey"), "{message}");
    // A malformed second key segment.
    let answer = get(&db, &format!("{MEMBERS}/team/not-a-uuid")).await;
    assert_error(&answer, StatusCode::BAD_REQUEST, "bad_request");

    // The list sorts by the key columns by default.
    let other = note_id(&db, "other").await;
    for team in ["b", "a"] {
        create(&db, MEMBERS, json!({"team": team, "note_id": other})).await;
    }
    let list = get(&db, &format!("{MEMBERS}?sort=team&order=asc")).await;
    assert_eq!(list.body["total"], 2, "{list:?}");
    assert_eq!(list.body["items"][0]["team"], "a");
    let answer = get(&db, &format!("{MEMBERS}?sort=role")).await;
    assert_error(&answer, StatusCode::BAD_REQUEST, "bad_request");
    db.drop().await;
}

#[actix_web::test]
async fn a_table_without_a_key_gets_list_and_create_under_its_route_base() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let answer = call(
        &db,
        Method::POST,
        "/api/v1/audit-log",
        Some(json!({"action": "login"})),
    )
    .await;
    assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
    assert_eq!(answer.location, None);
    assert_eq!(answer.body["action"], "login");
    create(&db, "/api/v1/audit-log", json!({"action": "logout"})).await;

    // Newest first.
    let list = get(&db, "/api/v1/audit-log").await;
    assert_eq!(list.body["total"], 2, "{list:?}");
    assert_eq!(list.body["items"][0]["action"], "logout");
    let list = get(&db, "/api/v1/audit-log?sort=created_at&order=asc").await;
    assert_eq!(list.body["items"][0]["action"], "login");
    // No search columns: `q` is an unknown parameter, ignored like any other.
    let list = get(&db, "/api/v1/audit-log?q=nothing-matches").await;
    assert_eq!(list.body["total"], 2, "{list:?}");

    // No `/{id}` routes, and not the default route base.
    assert_eq!(
        get(&db, "/api/v1/audit-log/1").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&db, "/api/v1/audit-events").await.status,
        StatusCode::NOT_FOUND
    );
    db.drop().await;
}

#[actix_web::test]
async fn a_skipped_endpoint_has_no_route() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let label = create(&db, "/api/v1/labels", json!({"name": "urgent"})).await;
    assert_eq!(label["id"], 1);
    let uri = "/api/v1/labels/1";
    assert_eq!(get(&db, uri).await.body, label);
    let answer = call(&db, Method::PATCH, uri, Some(json!({"name": "later"}))).await;
    assert_eq!(answer.body["name"], "later", "{answer:?}");
    // `remove` is skipped: the route answers only GET and PATCH.
    let answer = call(&db, Method::DELETE, uri, None).await;
    assert!(
        matches!(
            answer.status,
            StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
        ),
        "{answer:?}"
    );
    assert_eq!(get(&db, uri).await.status, StatusCode::OK);
    // `q` matches `name`.
    let list = get(&db, "/api/v1/labels?q=LAT").await;
    assert_eq!(list.body["total"], 1, "{list:?}");
    db.drop().await;
}

#[actix_web::test]
async fn the_users_service_override_runs_and_new_endpoints_work() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    // `drafts/service.rs` trims the title and refuses a blank one.
    let answer = call(
        &db,
        Method::POST,
        "/api/v1/drafts",
        Some(json!({"title": "  spaced  "})),
    )
    .await;
    assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
    assert_eq!(answer.body["title"], "spaced");
    let answer = call(
        &db,
        Method::POST,
        "/api/v1/drafts",
        Some(json!({"title": "   "})),
    )
    .await;
    assert_error(
        &answer,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
    );

    // The endpoints `0002_draft_keys.sql` added, with their defaults.
    let created = create(&db, "/api/v1/drafts", json!({"title": "keyed"})).await;
    let uri = format!(
        "/api/v1/drafts/{}",
        created["id"].as_str().unwrap_or_default()
    );
    assert_eq!(get(&db, &uri).await.body, created);
    let answer = call(&db, Method::PATCH, &uri, Some(json!({"title": "edited"}))).await;
    assert_eq!(answer.body["title"], "edited", "{answer:?}");
    let answer = call(&db, Method::DELETE, &uri, None).await;
    assert_eq!(answer.status, StatusCode::NO_CONTENT);
    assert_eq!(answer.body, Value::Null);
    db.drop().await;
}
