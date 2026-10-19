//! `/api/v1/users`, admin only: list, get, update and remove (a soft delete).
//! Each route's happy path and one failure.

mod support;

use actix_api::db::types::Role;
use actix_api::db::users::models::{NewUser, User};
use actix_api::db::users::{read, write};
use actix_web::http::{Method, StatusCode};
use serde_json::json;
use sqlx::PgPool;
use support::{Database, TestResult, admin, call, user};

/// Inserts a user with `email` and `display_name`.
async fn seed(pool: &PgPool, email: &str, display_name: Option<&str>) -> Result<User, sqlx::Error> {
    write::create(
        pool,
        NewUser {
            id: None,
            email: email.to_owned(),
            display_name: display_name.map(str::to_owned),
            role: None,
            is_active: None,
        },
    )
    .await
}

#[actix_web::test]
async fn an_admin_lists_users_matching_q_and_sorted() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    seed(&db.pool, "bob@example.com", Some("Bob")).await?;
    seed(&db.pool, "alice@example.com", Some("Alice")).await?;
    seed(&db.pool, "carol@example.com", None).await?;
    let token = admin()?;

    let answer = call(
        &db.pool,
        Some(&token),
        Method::GET,
        "/api/v1/users?q=EXAMPLE&sort=email&order=asc&per_page=2",
        None,
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(answer.body["total"], 3);
    assert_eq!(answer.body["page"], 1);
    assert_eq!(answer.body["per_page"], 2);
    let emails: Vec<&str> = answer.body["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["email"].as_str())
        .collect();
    assert_eq!(emails, ["alice@example.com", "bob@example.com"]);

    let answer = call(
        &db.pool,
        Some(&token),
        Method::GET,
        "/api/v1/users?q=car",
        None,
    )
    .await?;
    assert_eq!(answer.body["total"], 1, "{}", answer.body);
    db.drop().await
}

#[actix_web::test]
async fn a_user_who_is_not_an_admin_may_not_list_users() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let answer = call(&db.pool, Some(&user()?), Method::GET, "/api/v1/users", None).await?;
    assert_eq!(answer.status, StatusCode::FORBIDDEN);
    assert_eq!(answer.code(), "forbidden");
    db.drop().await
}

#[actix_web::test]
async fn an_admin_gets_a_user_without_its_hidden_columns() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let row = seed(&db.pool, "dana@example.com", Some("Dana")).await?;
    sqlx::query("UPDATE users SET password_hash = 'x', totp_secret = 's' WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await?;

    let uri = format!("/api/v1/users/{}", row.id);
    let answer = call(&db.pool, Some(&admin()?), Method::GET, &uri, None).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let mut fields: Vec<&String> = answer
        .body
        .as_object()
        .map(|body| body.keys().collect())
        .unwrap_or_default();
    fields.sort();
    assert_eq!(
        fields,
        [
            "created_at",
            "display_name",
            "email",
            "email_verified",
            "has_password",
            "id",
            "is_active",
            "role",
            "totp_enabled",
        ]
    );
    assert_eq!(answer.body["email"], "dana@example.com");
    assert_eq!(answer.body["role"], "user");
    assert_eq!(answer.body["has_password"], true);
    db.drop().await
}

#[actix_web::test]
async fn getting_a_missing_user_is_not_found() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let uri = format!("/api/v1/users/{}", uuid::Uuid::now_v7());
    let answer = call(&db.pool, Some(&admin()?), Method::GET, &uri, None).await?;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
    assert_eq!(answer.code(), "not_found");
    db.drop().await
}

#[actix_web::test]
async fn an_admin_updates_a_users_role_and_name() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let row = seed(&db.pool, "erin@example.com", Some("Erin")).await?;

    let uri = format!("/api/v1/users/{}", row.id);
    let body = json!({"role": "admin", "display_name": null, "is_active": false});
    let answer = call(&db.pool, Some(&admin()?), Method::PATCH, &uri, Some(body)).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(answer.body["role"], "admin");
    assert_eq!(answer.body["display_name"], serde_json::Value::Null);
    assert_eq!(answer.body["is_active"], false);
    assert_eq!(answer.body["email"], "erin@example.com");

    let stored = read::get_by_id(&db.pool, row.id).await?;
    assert_eq!(stored.map(|user| user.role), Some(Role::Admin));
    db.drop().await
}

#[actix_web::test]
async fn an_update_with_an_unknown_role_fails_validation() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let row = seed(&db.pool, "fay@example.com", None).await?;

    let uri = format!("/api/v1/users/{}", row.id);
    let body = json!({"role": "owner"});
    let answer = call(&db.pool, Some(&admin()?), Method::PATCH, &uri, Some(body)).await?;
    assert_eq!(
        answer.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        answer.body
    );
    assert_eq!(answer.code(), "validation_failed");
    db.drop().await
}

#[actix_web::test]
async fn an_admin_soft_deletes_a_user() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let row = seed(&db.pool, "gil@example.com", None).await?;
    let token = admin()?;

    let uri = format!("/api/v1/users/{}", row.id);
    let answer = call(&db.pool, Some(&token), Method::DELETE, &uri, None).await?;
    assert_eq!(answer.status, StatusCode::NO_CONTENT, "{}", answer.body);
    assert_eq!(answer.body, serde_json::Value::Null);

    let answer = call(&db.pool, Some(&token), Method::GET, &uri, None).await?;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
    // The row stays, marked deleted.
    let deleted: bool =
        sqlx::query_scalar("SELECT deleted_at IS NOT NULL FROM users WHERE id = $1")
            .bind(row.id)
            .fetch_one(&db.pool)
            .await?;
    assert!(deleted);
    db.drop().await
}

#[actix_web::test]
async fn removing_a_user_needs_a_token() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let row = seed(&db.pool, "hal@example.com", None).await?;

    let uri = format!("/api/v1/users/{}", row.id);
    let answer = call(&db.pool, None, Method::DELETE, &uri, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(answer.code(), "unauthorized");
    assert!(read::get_by_id(&db.pool, row.id).await?.is_some());
    db.drop().await
}
