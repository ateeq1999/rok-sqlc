//! The `unique` pre-check: a create or update body holding a value a unique
//! key already has answers `422 validation_failed` with the detail code
//! `unique`, every such field in one answer. Only live rows of the caller's
//! tenant count, the row being updated excepted; a key on `lower(<column>)`
//! compares ignoring case; a composite key is reported against its last
//! column. A race the check misses still hits the constraint: a `409`.

mod common;

use actix_web::ResponseError;
use actix_web::http::{Method, StatusCode};
use common::{Answer, Database, With, assert_error, call_with};
use rok_sqlc_api_app::error::AppError;
use rok_sqlc_api_app::shared::auth::Role;
use serde_json::{Value, json};
use uuid::Uuid;

const HANDLES: &str = "/api/v1/handles";
const NOTES: &str = "/api/v1/notes";
const FOLDERS: &str = "/api/v1/folders";
const TAGS: &str = "/api/v1/tags";

/// A signed-in caller in a tenant of their own.
fn tenant() -> With {
    With {
        token: Some(common::token(Uuid::now_v7(), Role::User)),
        tenant: Some(Uuid::now_v7()),
        if_match: None,
    }
}

/// A handle's body.
fn handle(email: &str, nickname: &str, region: &str, seat: i32) -> Value {
    json!({"email": email, "nickname": nickname, "region": region, "seat": seat})
}

/// `POST uri` with `body` as `with`.
async fn post(db: &Database, with: &With, uri: &str, body: Value) -> Answer {
    call_with(db, with, Method::POST, uri, Some(body)).await
}

/// The URI of the row `answer` created.
fn created(uri: &str, answer: &Answer) -> String {
    assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
    format!("{uri}/{}", answer.body["id"].as_str().expect("an id"))
}

/// Asserts `answer` is a `422` whose details hold, for `column`, the
/// `unique` failure with `message`.
fn assert_taken_as(answer: &Answer, column: &str, message: &str) {
    assert_error(answer, StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let detail = &answer.body["error"]["details"][column][0];
    assert_eq!(detail["code"], "unique", "{answer:?}");
    assert_eq!(detail["message"], message, "{answer:?}");
}

/// Asserts `answer` is a `422` whose details hold `column`'s `unique`
/// failure.
fn assert_taken(answer: &Answer, column: &str) {
    assert_taken_as(answer, column, &format!("{column} is already taken"));
}

#[actix_web::test]
async fn a_duplicate_fails_create_and_update() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let with = With::default();
    let taken = json!({"slug": "taken", "title": "Taken"});
    created(NOTES, &post(&db, &with, NOTES, taken).await);
    let again = json!({"slug": "taken", "title": "Again"});
    assert_taken(&post(&db, &with, NOTES, again).await, "slug");

    let other = json!({"slug": "other", "title": "Other"});
    let one = created(NOTES, &post(&db, &with, NOTES, other).await);
    let patch = Some(json!({"slug": "taken"}));
    let answer = call_with(&db, &with, Method::PATCH, &one, patch).await;
    assert_taken(&answer, "slug");
    db.drop().await;
}

#[actix_web::test]
async fn a_case_insensitive_key_ignores_case() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let with = tenant();
    let first = handle("Ann@Example.com", "ann", "eu", 1);
    created(HANDLES, &post(&db, &with, HANDLES, first).await);
    let second = handle("ann@example.COM", "anne", "eu", 2);
    assert_taken(&post(&db, &with, HANDLES, second).await, "email");
    db.drop().await;
}

#[actix_web::test]
async fn a_soft_deleted_row_does_not_count() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let with = tenant();
    let body = handle("gone@example.com", "gone", "eu", 1);
    let one = created(HANDLES, &post(&db, &with, HANDLES, body.clone()).await);
    let deleted = call_with(&db, &with, Method::DELETE, &one, None).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{deleted:?}");
    created(HANDLES, &post(&db, &with, HANDLES, body).await);
    db.drop().await;
}

#[actix_web::test]
async fn another_tenants_value_does_not_count() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let ours = handle("ours@example.com", "same", "eu", 1);
    created(HANDLES, &post(&db, &tenant(), HANDLES, ours).await);
    let theirs = handle("theirs@example.com", "same", "eu", 1);
    created(HANDLES, &post(&db, &tenant(), HANDLES, theirs).await);
    db.drop().await;
}

#[actix_web::test]
async fn a_composite_duplicate_is_reported_on_its_last_column() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let with = tenant();
    let first = handle("a@example.com", "a", "eu", 7);
    created(HANDLES, &post(&db, &with, HANDLES, first).await);
    // Another seat in the region, and the seat in another region, are free.
    let other_seat = handle("b@example.com", "b", "eu", 8);
    let one = created(HANDLES, &post(&db, &with, HANDLES, other_seat).await);
    let other_region = handle("c@example.com", "c", "us", 7);
    created(HANDLES, &post(&db, &with, HANDLES, other_region).await);

    let same = handle("d@example.com", "d", "eu", 7);
    let message = "region and seat are already taken together";
    assert_taken_as(&post(&db, &with, HANDLES, same).await, "seat", message);
    let patch = Some(json!({"region": "eu", "seat": 7}));
    let answer = call_with(&db, &with, Method::PATCH, &one, patch).await;
    assert_taken_as(&answer, "seat", message);
    db.drop().await;
}

#[actix_web::test]
async fn updating_a_row_to_its_own_values_is_allowed() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let with = tenant();
    let body = handle("own@example.com", "own", "eu", 1);
    let one = created(HANDLES, &post(&db, &with, HANDLES, body.clone()).await);
    let answer = call_with(&db, &with, Method::PATCH, &one, Some(body)).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    db.drop().await;
}

#[actix_web::test]
async fn every_taken_value_is_in_one_answer() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let with = tenant();
    let first = handle("all@example.com", "all", "eu", 1);
    created(HANDLES, &post(&db, &with, HANDLES, first.clone()).await);
    let answer = post(&db, &with, HANDLES, first).await;
    assert_taken(&answer, "email");
    assert_taken(&answer, "nickname");
    assert_taken_as(&answer, "seat", "region and seat are already taken together");
    let listed = call_with(&db, &with, Method::GET, HANDLES, None).await;
    assert_eq!(listed.body["total"], 1, "{listed:?}");
    db.drop().await;
}

#[actix_web::test]
async fn a_key_with_the_owner_column_is_checked_per_owner() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    // `folders`' `(owner_id, name)`: the owner comes from the caller, and
    // the failure is reported against `name`.
    let alice = tenant();
    let bob = tenant();
    let inbox = json!({"name": "Inbox"});
    created(FOLDERS, &post(&db, &alice, FOLDERS, inbox.clone()).await);
    created(FOLDERS, &post(&db, &bob, FOLDERS, inbox.clone()).await);
    assert_taken(&post(&db, &alice, FOLDERS, inbox).await, "name");

    let other = json!({"name": "Other"});
    let one = created(FOLDERS, &post(&db, &alice, FOLDERS, other).await);
    let patch = Some(json!({"name": "Inbox"}));
    let answer = call_with(&db, &alice, Method::PATCH, &one, patch).await;
    assert_taken(&answer, "name");
    db.drop().await;
}

#[actix_web::test]
async fn a_composite_key_with_a_lowered_column_ignores_its_case() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    // `tags`' `(tenant_id, lower(name))`.
    let ours = tenant();
    created(TAGS, &post(&db, &ours, TAGS, json!({"name": "Red"})).await);
    created(TAGS, &post(&db, &tenant(), TAGS, json!({"name": "red"})).await);
    assert_taken(&post(&db, &ours, TAGS, json!({"name": "RED"})).await, "name");
    db.drop().await;
}

/// The race the pre-check can't see: a write that still hits the unique
/// constraint is a `409 conflict` naming it.
#[actix_web::test]
async fn a_unique_violation_maps_to_a_conflict() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let insert = "INSERT INTO notes (id, slug, title) VALUES ($1, 'race', 'Race')";
    for _ in 0..2 {
        let result = sqlx::query(insert).bind(Uuid::now_v7()).execute(&db.pool).await;
        if let Err(error) = result {
            let error = AppError::from(error);
            assert_eq!(error.status_code(), StatusCode::CONFLICT, "{error:?}");
            assert_eq!(error.code(), "conflict");
            assert!(error.to_string().contains("notes_slug_key"), "{error}");
        }
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes")
        .fetch_one(&db.pool)
        .await
        .expect("a count");
    assert_eq!(count, 1);
    db.drop().await;
}
