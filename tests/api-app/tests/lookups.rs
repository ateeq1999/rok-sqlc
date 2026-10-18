//! Unique lookups (`GET /by-<column>/{value}`) and `POST /search` on the
//! public features, and both under `admin` access with a hidden column.

mod common;

use actix_web::http::{Method, StatusCode};
use common::{Database, assert_error, call, call_as, create, get, token};
use rok_sqlc_api_app::shared::auth::Role;
use serde_json::{Value, json};
use uuid::Uuid;

const NOTES: &str = "/api/v1/notes";

/// The `field` of each item of a page.
fn column(page: &Value, field: &str) -> Vec<Value> {
    page["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item[field].clone())
        .collect()
}

#[actix_web::test]
async fn a_unique_column_has_a_lookup() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let note = create(&db, NOTES, json!({"slug": "a b", "title": "First"})).await;
    let found = get(&db, &format!("{NOTES}/by-slug/a%20b")).await;
    assert_eq!(found.status, StatusCode::OK, "{found:?}");
    assert_eq!(found.body, note);
    let missing = get(&db, &format!("{NOTES}/by-slug/none")).await;
    assert_error(&missing, StatusCode::NOT_FOUND, "not_found");

    let label = create(&db, "/api/v1/labels", json!({"name": "red"})).await;
    let found = get(&db, "/api/v1/labels/by-name/red").await;
    assert_eq!(found.body, label);
    db.drop().await;
}

#[actix_web::test]
async fn a_lookup_takes_the_features_access() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let admin = token(Uuid::now_v7(), Role::Admin);
    let body = Some(json!({"email": "a@example.com", "password_hash": "hash"}));
    let created = call_as(&db, Some(&admin), Method::POST, "/api/v1/accounts", body).await;
    assert_eq!(created.status, StatusCode::CREATED, "{created:?}");
    let uri = "/api/v1/accounts/by-email/a@example.com";
    let found = call_as(&db, Some(&admin), Method::GET, uri, None).await;
    assert_eq!(found.status, StatusCode::OK, "{found:?}");
    assert_eq!(found.body, created.body);
    assert!(found.body.get("password_hash").is_none());
    assert_error(&get(&db, uri).await, StatusCode::UNAUTHORIZED, "unauthorized");
    let user = token(Uuid::now_v7(), Role::User);
    let forbidden = call_as(&db, Some(&user), Method::GET, uri, None).await;
    assert_error(&forbidden, StatusCode::FORBIDDEN, "forbidden");
    db.drop().await;
}

#[actix_web::test]
async fn search_filters_sorts_and_pages_like_list() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    for (slug, title, priority) in [("a", "alpha", 3), ("b", "beta", 1), ("c", "gamma", 2)] {
        create(
            &db,
            NOTES,
            json!({"slug": slug, "title": title, "priority": priority}),
        )
        .await;
    }
    let search = format!("{NOTES}/search");
    let body = json!({
        "filter": {"title": [{"contains": "a"}], "priority": [{"ge": 2}]},
        "sort": "priority",
        "order": "asc",
    });
    let page = call(&db, Method::POST, &search, Some(body)).await;
    assert_eq!(page.status, StatusCode::OK, "{page:?}");
    assert_eq!(column(&page.body, "slug"), [json!("c"), json!("a")]);
    assert_eq!(page.body["total"], 2);
    assert_eq!(page.body["page"], 1);
    assert_eq!(page.body["per_page"], 20);

    // An empty filter matches every row, one page at a time.
    let all = json!({"sort": "title"});
    let all = call(&db, Method::POST, &search, Some(all)).await;
    assert_eq!(column(&all.body, "slug"), [json!("c"), json!("b"), json!("a")]);
    let second = json!({"sort": "title", "page": 2, "per_page": 2});
    let second = call(&db, Method::POST, &search, Some(second)).await;
    assert_eq!(column(&second.body, "slug"), [json!("a")]);
    assert_eq!(second.body["total"], 3);

    for body in [
        json!({"filter": {"nope": []}}),
        json!({"filter": {"members": {}}}),
        json!({"sort": "body"}),
        json!({"filter": {"priority": [{"ge": "x"}]}}),
    ] {
        let answer = call(&db, Method::POST, &search, Some(body)).await;
        assert_error(&answer, StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    }
    db.drop().await;
}

#[actix_web::test]
async fn search_rejects_a_hidden_column_by_name() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let admin = token(Uuid::now_v7(), Role::Admin);
    let search = "/api/v1/accounts/search";
    let body = Some(json!({"filter": {"password_hash": [{"eq": "x"}]}}));
    let answer = call_as(&db, Some(&admin), Method::POST, search, body).await;
    assert_error(&answer, StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert!(
        answer.body["error"]["details"]
            .to_string()
            .contains("password_hash"),
        "{answer:?}"
    );
    let body = Some(json!({"sort": "password_hash"}));
    let answer = call_as(&db, Some(&admin), Method::POST, search, body).await;
    assert_error(&answer, StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert!(
        answer.body["error"]["details"].to_string().contains("sort"),
        "{answer:?}"
    );
    let answer = call(&db, Method::POST, search, Some(json!({}))).await;
    assert_error(&answer, StatusCode::UNAUTHORIZED, "unauthorized");
    db.drop().await;
}

#[actix_web::test]
async fn search_sees_only_the_callers_rows() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let mut tokens = Vec::new();
    for _ in 0..2 {
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO accounts (id, email, password_hash) VALUES ($1, $2, 'hash')")
            .bind(id)
            .bind(format!("{id}@example.com"))
            .execute(&db.pool)
            .await
            .expect("an account");
        let token = token(id, Role::User);
        let body = Some(json!({"title": "same"}));
        let answer = call_as(&db, Some(&token), Method::POST, "/api/v1/tasks", body).await;
        assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
        tokens.push(token);
    }
    let body = Some(json!({"filter": {"title": [{"eq": "same"}]}}));
    let answer = call_as(&db, Some(&tokens[0]), Method::POST, "/api/v1/tasks/search", body).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.body["total"], 1);
    db.drop().await;
}
