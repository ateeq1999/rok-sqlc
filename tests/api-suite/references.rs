//! The `exists` pre-check: a create or update body naming, by a foreign
//! key, a row the caller can't see answers `422 validation_failed` with
//! the detail code `exists`, every such field in one answer. A row is
//! visible when it exists, is live (not soft-deleted), is in the caller's
//! tenant and, for an owned table, is the caller's. `null` refers to
//! nothing and passes.

use crate::common;

use common::{Answer, Database, With, assert_error, call, call_with, create};
use common::{Method, StatusCode, api_test};
use rok_sqlc_api_app::shared::auth::Role;
use serde_json::{Value, json};
use uuid::Uuid;

const DOCUMENTS: &str = "/api/v1/documents";
const COMMENTS: &str = "/api/v1/comments";
const TASKS: &str = "/api/v1/tasks";
const BOOKMARKS: &str = "/api/v1/bookmarks";

/// An admin in `tenant` with an account, so their tasks can be created.
async fn admin(db: &Database, tenant: Uuid) -> With {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO accounts (id, email, password_hash) VALUES ($1, $2, 'x')")
        .bind(id)
        .bind(format!("{id}@example.com"))
        .execute(&db.pool)
        .await
        .expect("an account");
    With {
        token: Some(common::token(id, Role::Admin)),
        tenant: Some(tenant),
        if_match: None,
    }
}

/// `with`, sending `If-Match: "1"`.
fn at_first_version(with: &With) -> With {
    With {
        if_match: Some("\"1\"".to_owned()),
        ..with.clone()
    }
}

/// `POST uri` with `body` as `with`.
async fn post(db: &Database, with: &With, uri: &str, body: Value) -> Answer {
    call_with(db, with, Method::POST, uri, Some(body)).await
}

/// The id of the row `answer` created.
fn id(answer: &Answer) -> String {
    assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
    answer.body["id"].as_str().expect("an id").to_owned()
}

/// Asserts `answer` is a `422` whose details hold, for `column`, the
/// `exists` failure naming `singular`.
fn assert_missing(answer: &Answer, column: &str, singular: &str) {
    assert_error(
        answer,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
    );
    let detail = &answer.body["error"]["details"][column][0];
    assert_eq!(detail["code"], "exists", "{answer:?}");
    assert_eq!(
        detail["message"],
        format!("{column} does not refer to an existing {singular}"),
        "{answer:?}"
    );
}

#[api_test]
async fn a_missing_reference_fails_create_with_every_field_named() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    // A public table referencing a plain one.
    let body = json!({"team": "x", "note_id": Uuid::now_v7()});
    let answer = call(&db, Method::POST, "/api/v1/members", Some(body)).await;
    assert_missing(&answer, "note_id", "note");

    // Both of a document's references, in one answer, and nothing written.
    let alice = admin(&db, Uuid::now_v7()).await;
    let body = json!({"code": "a", "title": "A", "label_id": 999_999, "task_id": Uuid::now_v7()});
    let answer = post(&db, &alice, DOCUMENTS, body).await;
    assert_missing(&answer, "label_id", "label");
    assert_missing(&answer, "task_id", "task");
    let listed = call_with(&db, &alice, Method::GET, DOCUMENTS, None).await;
    assert_eq!(listed.body["total"], 0, "{listed:?}");
    db.drop().await;
}

#[api_test]
async fn a_missing_reference_fails_update() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let alice = admin(&db, Uuid::now_v7()).await;
    let label = create(&db, "/api/v1/labels", json!({"name": "red"})).await;
    let body = json!({"code": "a", "title": "A", "label_id": label["id"]});
    let one = format!(
        "{DOCUMENTS}/{}",
        id(&post(&db, &alice, DOCUMENTS, body).await)
    );
    let patch = Some(json!({"label_id": 999_999}));
    let answer = call_with(&db, &at_first_version(&alice), Method::PATCH, &one, patch).await;
    assert_missing(&answer, "label_id", "label");
    let row = call_with(&db, &alice, Method::GET, &one, None).await;
    assert_eq!(row.body["label_id"], label["id"], "{row:?}");
    db.drop().await;
}

#[api_test]
async fn another_owners_row_is_missing() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let tenant = Uuid::now_v7();
    let alice = admin(&db, tenant).await;
    let bob = admin(&db, tenant).await;
    let theirs = id(&post(&db, &bob, TASKS, json!({"title": "Bob's"})).await);
    let mine = id(&post(&db, &alice, TASKS, json!({"title": "Alice's"})).await);

    let body = json!({"code": "a", "title": "A", "task_id": theirs});
    assert_missing(&post(&db, &alice, DOCUMENTS, body).await, "task_id", "task");
    let body = json!({"code": "a", "title": "A", "task_id": mine});
    let created = post(&db, &alice, DOCUMENTS, body).await;
    assert_eq!(created.body["task_id"], mine, "{created:?}");
    db.drop().await;
}

#[api_test]
async fn another_tenants_row_is_missing() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let ours = admin(&db, Uuid::now_v7()).await;
    let theirs = admin(&db, Uuid::now_v7()).await;
    let body = json!({"code": "a", "title": "A"});
    let their_document = id(&post(&db, &theirs, DOCUMENTS, body.clone()).await);
    let our_document = id(&post(&db, &ours, DOCUMENTS, body).await);

    let comment = |document: &str| json!({"document_id": document, "body": "Hi"});
    let answer = post(&db, &ours, COMMENTS, comment(&their_document)).await;
    assert_missing(&answer, "document_id", "document");
    let one = format!(
        "{COMMENTS}/{}",
        id(&post(&db, &ours, COMMENTS, comment(&our_document)).await)
    );
    let patch = Some(comment(&their_document));
    let answer = call_with(&db, &ours, Method::PATCH, &one, patch).await;
    assert_missing(&answer, "document_id", "document");
    db.drop().await;
}

#[api_test]
async fn a_soft_deleted_row_is_missing() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let alice = admin(&db, Uuid::now_v7()).await;
    let body = json!({"code": "a", "title": "A"});
    let document = id(&post(&db, &alice, DOCUMENTS, body).await);
    let one = format!("{DOCUMENTS}/{document}");
    let deleted = call_with(&db, &alice, Method::DELETE, &one, None).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{deleted:?}");

    let body = json!({"document_id": document, "body": "Hi"});
    let answer = post(&db, &alice, COMMENTS, body).await;
    assert_missing(&answer, "document_id", "document");
    db.drop().await;
}

#[api_test]
async fn a_null_reference_is_allowed() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let alice = admin(&db, Uuid::now_v7()).await;
    let task = id(&post(&db, &alice, TASKS, json!({"title": "T"})).await);
    let body = json!({"code": "a", "title": "A", "label_id": null, "task_id": task});
    let created = post(&db, &alice, DOCUMENTS, body).await;
    assert!(created.body["label_id"].is_null(), "{created:?}");
    let one = format!("{DOCUMENTS}/{}", id(&created));
    let patch = Some(json!({"task_id": null, "label_id": null}));
    let updated = call_with(&db, &at_first_version(&alice), Method::PATCH, &one, patch).await;
    assert_eq!(updated.status, StatusCode::OK, "{updated:?}");
    assert!(updated.body["task_id"].is_null(), "{updated:?}");
    db.drop().await;
}

#[api_test]
async fn a_feature_without_an_owner_sees_only_the_callers_rows() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    // `bookmarks` has neither an owner nor a tenant column, but its caller
    // is signed in: another owner's document is missing too.
    let tenant = Uuid::now_v7();
    let alice = admin(&db, tenant).await;
    let bob = admin(&db, tenant).await;
    let body = |code: &str| json!({"code": code, "title": "A"});
    let theirs = id(&post(&db, &bob, DOCUMENTS, body("b")).await);
    let mine = id(&post(&db, &alice, DOCUMENTS, body("a")).await);

    let bookmark = |document: &str| json!({"document_id": document, "note": "Read"});
    let answer = post(&db, &alice, BOOKMARKS, bookmark(&theirs)).await;
    assert_missing(&answer, "document_id", "document");
    let one = format!(
        "{BOOKMARKS}/{}",
        id(&post(&db, &alice, BOOKMARKS, bookmark(&mine)).await)
    );
    let patch = Some(json!({"document_id": theirs}));
    let answer = call_with(&db, &alice, Method::PATCH, &one, patch).await;
    assert_missing(&answer, "document_id", "document");
    db.drop().await;
}

#[api_test]
async fn a_feature_without_a_tenant_sees_only_the_callers_tenant() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    // The same caller's document, in another tenant.
    let alice = admin(&db, Uuid::now_v7()).await;
    let elsewhere = With {
        tenant: Some(Uuid::now_v7()),
        ..alice.clone()
    };
    let body = json!({"code": "a", "title": "A"});
    let document = id(&post(&db, &elsewhere, DOCUMENTS, body).await);

    let bookmark = json!({"document_id": document, "note": "Read"});
    let answer = post(&db, &alice, BOOKMARKS, bookmark.clone()).await;
    assert_missing(&answer, "document_id", "document");
    let created = post(&db, &elsewhere, BOOKMARKS, bookmark).await;
    assert_eq!(created.body["document_id"], document, "{created:?}");
    db.drop().await;
}
