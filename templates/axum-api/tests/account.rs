//! The signed-in user's own account: `GET /auth/me`, `GET /users/me` and
//! `PATCH /users/me`, open to any signed-in user (not only admins), and
//! never taken for `/users/{id}`.

mod support;

use axum::http::{Method, StatusCode};
use serde_json::json;
use support::{Database, TestResult, call, user};

const ME: &str = "/api/v1/users/me";

#[tokio::test]
async fn a_user_reads_their_own_account_at_both_paths() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let token = user(&db.pool).await?;
    let answer = call(&db.pool, Some(&token), Method::GET, ME, None).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(answer.body["role"], "user");
    assert!(
        answer.body.get("token_version").is_none(),
        "{}",
        answer.body
    );

    let other = call(&db.pool, Some(&token), Method::GET, "/api/v1/auth/me", None).await?;
    assert_eq!(other.status, StatusCode::OK, "{}", other.body);
    assert_eq!(other.body, answer.body);

    let answer = call(&db.pool, None, Method::GET, ME, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    db.drop().await
}

#[tokio::test]
async fn a_user_sets_and_clears_their_display_name() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let token = user(&db.pool).await?;
    let body = Some(json!({"display_name": "  Ada Lovelace "}));
    let answer = call(&db.pool, Some(&token), Method::PATCH, ME, body).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(answer.body["display_name"], "Ada Lovelace");

    // Left out, it stays.
    let answer = call(&db.pool, Some(&token), Method::PATCH, ME, Some(json!({}))).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(answer.body["display_name"], "Ada Lovelace");

    let body = Some(json!({"display_name": null}));
    let answer = call(&db.pool, Some(&token), Method::PATCH, ME, body).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(answer.body["display_name"], serde_json::Value::Null);

    let answer = call(&db.pool, Some(&token), Method::GET, ME, None).await?;
    assert_eq!(answer.body["display_name"], serde_json::Value::Null);
    db.drop().await
}

#[tokio::test]
async fn a_display_name_too_long_or_of_the_wrong_type_is_a_validation_failure() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let token = user(&db.pool).await?;
    let body = Some(json!({"display_name": "x".repeat(101)}));
    let answer = call(&db.pool, Some(&token), Method::PATCH, ME, body).await?;
    assert_eq!(
        answer.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        answer.body
    );
    assert!(
        answer.body["error"]["details"]["display_name"].is_array(),
        "{}",
        answer.body
    );

    let body = Some(json!({"display_name": 5}));
    let answer = call(&db.pool, Some(&token), Method::PATCH, ME, body).await?;
    assert_eq!(
        answer.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        answer.body
    );
    db.drop().await
}
