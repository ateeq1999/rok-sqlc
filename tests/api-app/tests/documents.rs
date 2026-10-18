//! Tenants, versions, soft delete, and ownership under `admin` access:
//! `/api/v1/documents` is tenant-scoped, versioned by `revision`,
//! soft-deleted through `deleted_at`, admin only, owned through `owner_id`,
//! and hides `secret`.

mod common;

use actix_web::http::{Method, StatusCode};
use common::{Answer, Database, With, assert_error, call_with};
use rok_sqlc_api_app::shared::auth::Role;
use serde_json::{Value, json};
use uuid::Uuid;

const DOCUMENTS: &str = "/api/v1/documents";

/// An admin in `tenant`.
fn admin(tenant: Uuid) -> With {
    With {
        token: Some(common::token(Uuid::now_v7(), Role::Admin)),
        tenant: Some(tenant),
        if_match: None,
    }
}

/// `with`, sending `If-Match: <version>`.
fn matching(with: &With, version: &str) -> With {
    With {
        if_match: Some(version.to_owned()),
        ..with.clone()
    }
}

/// Creates a document as `with`, answering `201`; returns its body.
async fn document(db: &Database, with: &With, code: &str) -> Value {
    let body = json!({"code": code, "title": "Draft", "secret": format!("s-{code}")});
    let answer = call_with(db, with, Method::POST, DOCUMENTS, Some(body)).await;
    assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
    assert_eq!(answer.etag.as_deref(), Some("\"1\""), "{answer:?}");
    answer.body
}

async fn get(db: &Database, with: &With, uri: &str) -> Answer {
    call_with(db, with, Method::GET, uri, None).await
}

#[actix_web::test]
async fn create_sets_tenant_and_owner_and_hides_the_secret() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let tenant = Uuid::now_v7();
    let id = Uuid::now_v7();
    let alice = With {
        token: Some(common::token(id, Role::Admin)),
        tenant: Some(tenant),
        if_match: None,
    };
    let row = document(&db, &alice, "a").await;
    assert_eq!(row["tenant_id"], tenant.to_string());
    assert_eq!(row["owner_id"], id.to_string());
    assert!(row.get("secret").is_none(), "{row}");
    assert_eq!(row["revision"], 1);
    assert!(row["deleted_at"].is_null());
    db.drop().await;
}

#[actix_web::test]
async fn a_missing_tenant_is_a_wiring_bug() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let mut no_tenant = admin(Uuid::now_v7());
    no_tenant.tenant = None;
    let answer = get(&db, &no_tenant, DOCUMENTS).await;
    assert_error(&answer, StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    db.drop().await;
}

#[actix_web::test]
async fn rows_are_scoped_to_the_tenant_and_the_owning_admin() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let tenant = Uuid::now_v7();
    let alice = admin(tenant);
    let bob = admin(tenant);
    let elsewhere = With {
        tenant: Some(Uuid::now_v7()),
        ..alice.clone()
    };
    let row = document(&db, &alice, "a").await;
    document(&db, &bob, "b").await;
    let one = format!("{DOCUMENTS}/{}", row["id"].as_str().expect("an id"));

    let fetched = get(&db, &alice, &one).await;
    assert_eq!(fetched.status, StatusCode::OK, "{fetched:?}");
    assert_eq!(fetched.etag.as_deref(), Some("\"1\""));
    assert_error(&get(&db, &bob, &one).await, StatusCode::NOT_FOUND, "not_found");
    assert_error(
        &get(&db, &elsewhere, &one).await,
        StatusCode::NOT_FOUND,
        "not_found",
    );
    let list = get(&db, &alice, DOCUMENTS).await;
    assert_eq!(list.body["total"], 1, "{list:?}");
    assert_eq!(list.body["items"][0]["code"], "a");
    assert_eq!(get(&db, &elsewhere, DOCUMENTS).await.body["total"], 0);

    // A user is no admin.
    let user = With {
        token: Some(common::token(Uuid::now_v7(), Role::User)),
        ..alice.clone()
    };
    assert_error(&get(&db, &user, &one).await, StatusCode::FORBIDDEN, "forbidden");
    db.drop().await;
}

#[actix_web::test]
async fn lookups_by_a_unique_column_are_scoped_and_hidden_ones_have_none() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let tenant = Uuid::now_v7();
    let alice = admin(tenant);
    let bob = admin(tenant);
    document(&db, &alice, "a/b c").await;
    let found = get(&db, &alice, &format!("{DOCUMENTS}/by-code/a%2Fb%20c")).await;
    assert_eq!(found.status, StatusCode::OK, "{found:?}");
    assert_eq!(found.body["code"], "a/b c");
    assert_eq!(found.etag.as_deref(), Some("\"1\""));
    let other = get(&db, &bob, &format!("{DOCUMENTS}/by-code/a%2Fb%20c")).await;
    assert_error(&other, StatusCode::NOT_FOUND, "not_found");
    let missing = get(&db, &alice, &format!("{DOCUMENTS}/by-code/nope")).await;
    assert_error(&missing, StatusCode::NOT_FOUND, "not_found");
    // `secret` is unique but hidden: no route.
    let hidden = get(&db, &alice, &format!("{DOCUMENTS}/by-secret/s-a")).await;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND, "{hidden:?}");
    assert!(hidden.body.get("code").is_none(), "{hidden:?}");
    db.drop().await;
}

#[actix_web::test]
async fn update_needs_the_current_version_in_if_match() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let tenant = Uuid::now_v7();
    let alice = admin(tenant);
    let row = document(&db, &alice, "a").await;
    let one = format!("{DOCUMENTS}/{}", row["id"].as_str().expect("an id"));
    let patch = || Some(json!({"title": "Final"}));

    let missing = call_with(&db, &alice, Method::PATCH, &one, patch()).await;
    assert_error(
        &missing,
        StatusCode::PRECONDITION_REQUIRED,
        "precondition_required",
    );
    for stale in ["\"0\"", "\"2\"", "garbage"] {
        let answer = call_with(&db, &matching(&alice, stale), Method::PATCH, &one, patch()).await;
        assert_error(&answer, StatusCode::PRECONDITION_FAILED, "version_conflict");
    }
    let updated = call_with(&db, &matching(&alice, "\"1\""), Method::PATCH, &one, patch()).await;
    assert_eq!(updated.status, StatusCode::OK, "{updated:?}");
    assert_eq!(updated.body["title"], "Final");
    assert_eq!(updated.etag.as_deref(), Some("\"2\""));
    let again = call_with(&db, &matching(&alice, "\"1\""), Method::PATCH, &one, patch()).await;
    assert_error(&again, StatusCode::PRECONDITION_FAILED, "version_conflict");

    // Another admin's row stays a 404, whatever the version.
    let bob = matching(&admin(tenant), "\"2\"");
    let theirs = call_with(&db, &bob, Method::PATCH, &one, patch()).await;
    assert_error(&theirs, StatusCode::NOT_FOUND, "not_found");
    assert_eq!(get(&db, &alice, &one).await.body["title"], "Final");
    db.drop().await;
}

#[actix_web::test]
async fn delete_soft_deletes_restore_undoes_it_and_permanent_purges() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let tenant = Uuid::now_v7();
    let alice = admin(tenant);
    let bob = admin(tenant);
    let row = document(&db, &alice, "a").await;
    let one = format!("{DOCUMENTS}/{}", row["id"].as_str().expect("an id"));
    let restore = format!("{one}/restore");

    let deleted = call_with(&db, &alice, Method::DELETE, &one, None).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{deleted:?}");
    assert_error(&get(&db, &alice, &one).await, StatusCode::NOT_FOUND, "not_found");
    assert_eq!(get(&db, &alice, DOCUMENTS).await.body["total"], 0);
    let gone = call_with(&db, &alice, Method::DELETE, &one, None).await;
    assert_error(&gone, StatusCode::NOT_FOUND, "not_found");

    // Only the owner restores it.
    let theirs = call_with(&db, &bob, Method::POST, &restore, None).await;
    assert_error(&theirs, StatusCode::NOT_FOUND, "not_found");
    let restored = call_with(&db, &alice, Method::POST, &restore, None).await;
    assert_eq!(restored.status, StatusCode::OK, "{restored:?}");
    assert_eq!(restored.body["code"], "a");
    assert!(restored.body["deleted_at"].is_null());
    assert!(restored.etag.is_some());
    assert_eq!(get(&db, &alice, &one).await.status, StatusCode::OK);
    let live = call_with(&db, &alice, Method::POST, &restore, None).await;
    assert_error(&live, StatusCode::NOT_FOUND, "not_found");

    let permanent = format!("{one}?permanent=true");
    let theirs = call_with(&db, &bob, Method::DELETE, &permanent, None).await;
    assert_error(&theirs, StatusCode::NOT_FOUND, "not_found");
    let purged = call_with(&db, &alice, Method::DELETE, &permanent, None).await;
    assert_eq!(purged.status, StatusCode::NO_CONTENT, "{purged:?}");
    let after = call_with(&db, &alice, Method::POST, &restore, None).await;
    assert_error(&after, StatusCode::NOT_FOUND, "not_found");
    db.drop().await;
}

#[actix_web::test]
async fn search_is_scoped_and_rejects_hidden_columns() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let tenant = Uuid::now_v7();
    let alice = admin(tenant);
    let bob = admin(tenant);
    for code in ["a", "b", "c"] {
        document(&db, &alice, code).await;
    }
    document(&db, &bob, "d").await;
    let search = format!("{DOCUMENTS}/search");
    let body = json!({
        "filter": {"code": [{"in": ["a", "b", "d"]}]},
        "sort": "created_at",
        "order": "asc",
        "per_page": 1,
    });
    let page = call_with(&db, &alice, Method::POST, &search, Some(body)).await;
    assert_eq!(page.status, StatusCode::OK, "{page:?}");
    assert_eq!(page.body["total"], 2, "{page:?}");
    assert_eq!(page.body["per_page"], 1);
    assert_eq!(page.body["items"][0]["code"], "a");
    assert!(page.body["items"][0].get("secret").is_none());

    let hidden = json!({"filter": {"secret": [{"eq": "s-a"}]}});
    let answer = call_with(&db, &alice, Method::POST, &search, Some(hidden)).await;
    assert_error(&answer, StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert!(answer.body.to_string().contains("secret"), "{answer:?}");
    let sorted = json!({"sort": "secret"});
    let answer = call_with(&db, &alice, Method::POST, &search, Some(sorted)).await;
    assert_error(&answer, StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert!(answer.body.to_string().contains("sort"), "{answer:?}");
    db.drop().await;
}

#[actix_web::test]
async fn relationships_reach_documents_only_in_the_tenant_and_for_their_owner() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let tenant = Uuid::now_v7();
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO accounts (id, email, password_hash) VALUES ($1, 'a@x', 'h')")
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("an account");
    let alice = With {
        token: Some(common::token(id, Role::Admin)),
        tenant: Some(tenant),
        if_match: None,
    };
    let label = common::create(&db, "/api/v1/labels", json!({"name": "red"})).await;
    let task = call_with(&db, &alice, Method::POST, "/api/v1/tasks", Some(json!({"title": "t"})))
        .await
        .body;
    let body = json!({
        "code": "a",
        "title": "Draft",
        "label_id": label["id"],
        "task_id": task["id"],
    });
    let created = call_with(&db, &alice, Method::POST, DOCUMENTS, Some(body)).await;
    assert_eq!(created.status, StatusCode::CREATED, "{created:?}");
    let one = format!("{DOCUMENTS}/{}", created.body["id"].as_str().expect("an id"));

    // Belongs-to, embedded.
    let included = get(&db, &alice, &format!("{one}?include=label")).await;
    assert_eq!(included.body["label"]["name"], "red", "{included:?}");
    assert_eq!(included.etag.as_deref(), Some("\"1\""));
    let included = get(&db, &alice, &format!("{one}?include=task")).await;
    assert_eq!(included.body["task"]["title"], "t", "{included:?}");

    // Has-one, from the task: `documents` is admin only and tenant-scoped.
    let task_uri = format!("/api/v1/tasks/{}?include=document", task["id"].as_str().expect("an id"));
    let found = get(&db, &alice, &task_uri).await;
    assert_eq!(found.status, StatusCode::OK, "{found:?}");
    assert_eq!(found.body["document"]["code"], "a");
    let elsewhere = With {
        tenant: Some(Uuid::now_v7()),
        ..alice.clone()
    };
    assert!(get(&db, &elsewhere, &task_uri).await.body["document"].is_null());
    let no_tenant = With {
        tenant: None,
        ..alice.clone()
    };
    let answer = get(&db, &no_tenant, &task_uri).await;
    assert_error(&answer, StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    let as_user = With {
        token: Some(common::token(id, Role::User)),
        ..alice.clone()
    };
    assert_error(&get(&db, &as_user, &task_uri).await, StatusCode::FORBIDDEN, "forbidden");

    // Has-many, from the public label: only the owning admin, in the tenant.
    let documents = format!("/api/v1/labels/{}/documents", label["id"]);
    let mine = get(&db, &alice, &documents).await;
    assert_eq!(mine.body[0]["code"], "a", "{mine:?}");
    assert_eq!(get(&db, &elsewhere, &documents).await.body, json!([]));
    let bob = admin(tenant);
    assert_eq!(get(&db, &bob, &documents).await.body, json!([]));
    let anonymous = common::get(&db, &documents).await;
    assert_error(&anonymous, StatusCode::UNAUTHORIZED, "unauthorized");
    db.drop().await;
}
