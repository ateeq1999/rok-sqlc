//! Signing in: `POST /auth/login` answers a session for the right
//! password of an active, verified account; locks an account after five
//! wrong passwords; and every `/auth` route is rate limited per IP.

mod support;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use support::{Database, PASSWORD, TestResult, call, call_with, register_verified, state_with};

const REGISTER: &str = "/api/v1/auth/register";
const LOGIN: &str = "/api/v1/auth/login";

fn credentials(email: &str, password: &str) -> Option<Value> {
    Some(json!({"email": email, "password": password}))
}

#[tokio::test]
async fn an_unverified_email_is_forbidden_unless_verification_is_off() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let body = credentials("ada@example.com", PASSWORD);
    let answer = call(&db.pool, None, Method::POST, REGISTER, body.clone()).await?;
    assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.body);

    let answer = call(&db.pool, None, Method::POST, LOGIN, body.clone()).await?;
    assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.body);
    assert_eq!(answer.code(), "forbidden");

    let state = state_with(&db.pool, &[("REQUIRE_VERIFIED_EMAIL", "false")])?;
    let answer = call_with(&state, None, Method::POST, LOGIN, body).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert!(answer.body["access_token"].is_string(), "{}", answer.body);
    db.drop().await
}

#[tokio::test]
async fn a_wrong_password_or_an_unknown_email_is_unauthorized() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    register_verified(&db.pool, "ada@example.com", PASSWORD).await?;
    let body = credentials("ada@example.com", "not the password");
    let answer = call(&db.pool, None, Method::POST, LOGIN, body).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    let wrong_password = answer.body["error"]["message"].clone();

    let body = credentials("nobody@example.com", PASSWORD);
    let answer = call(&db.pool, None, Method::POST, LOGIN, body).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    assert_eq!(answer.body["error"]["message"], wrong_password);
    db.drop().await
}

#[tokio::test]
async fn a_disabled_account_with_the_right_password_is_forbidden() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    register_verified(&db.pool, "ada@example.com", PASSWORD).await?;
    sqlx::query("UPDATE users SET is_active = false")
        .execute(&db.pool)
        .await?;
    let body = credentials("ada@example.com", PASSWORD);
    let answer = call(&db.pool, None, Method::POST, LOGIN, body).await?;
    assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.body);

    let body = credentials("ada@example.com", "not the password");
    let answer = call(&db.pool, None, Method::POST, LOGIN, body).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    db.drop().await
}

#[tokio::test]
async fn five_wrong_passwords_lock_the_account_until_the_lock_passes() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    register_verified(&db.pool, "ada@example.com", PASSWORD).await?;
    let wrong = credentials("ada@example.com", "not the password");
    let right = credentials("ada@example.com", PASSWORD);

    // Four failures, then a success: the count starts over.
    for _ in 0..4 {
        call(&db.pool, None, Method::POST, LOGIN, wrong.clone()).await?;
    }
    let answer = call(&db.pool, None, Method::POST, LOGIN, right.clone()).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);

    for _ in 0..5 {
        let answer = call(&db.pool, None, Method::POST, LOGIN, wrong.clone()).await?;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    }
    let answer = call(&db.pool, None, Method::POST, LOGIN, right.clone()).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);

    // Fifteen minutes later (the lock moved into the past):
    sqlx::query("UPDATE users SET locked_until = now() - interval '1 second'")
        .execute(&db.pool)
        .await?;
    let answer = call(&db.pool, None, Method::POST, LOGIN, right).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    db.drop().await
}

#[tokio::test]
async fn the_lock_lasts_fifteen_minutes() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    register_verified(&db.pool, "ada@example.com", PASSWORD).await?;
    let wrong = credentials("ada@example.com", "not the password");
    for _ in 0..5 {
        call(&db.pool, None, Method::POST, LOGIN, wrong.clone()).await?;
    }
    let (minutes,): (f64,) =
        sqlx::query_as("SELECT (extract(epoch FROM locked_until - now()) / 60)::float8 FROM users")
            .fetch_one(&db.pool)
            .await?;
    assert!((14.0..=15.0).contains(&minutes), "{minutes}");
    db.drop().await
}

#[tokio::test]
async fn the_auth_routes_answer_429_past_the_rate_limit() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let state = state_with(&db.pool, &[("AUTH_RATE_LIMIT", "3")])?;
    let body = credentials("nobody@example.com", PASSWORD);
    for _ in 0..3 {
        let answer = call_with(&state, None, Method::POST, LOGIN, body.clone()).await?;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    }
    let answer = call_with(&state, None, Method::POST, LOGIN, body).await?;
    assert_eq!(
        answer.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        answer.body
    );
    assert_eq!(answer.code(), "too_many_requests");

    // `/auth/me` counts against the same limit; `/users/me` doesn't.
    let answer = call_with(&state, None, Method::GET, "/api/v1/auth/me", None).await?;
    assert_eq!(
        answer.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        answer.body
    );
    let answer = call_with(&state, None, Method::GET, "/api/v1/users/me", None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    db.drop().await
}

#[tokio::test]
async fn every_request_under_auth_counts_against_the_rate_limit() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let state = state_with(&db.pool, &[("AUTH_RATE_LIMIT", "3")])?;
    // An unknown path under `/auth` is the standard `404`, and counts.
    for _ in 0..2 {
        let answer = call_with(&state, None, Method::GET, "/api/v1/auth/nothing", None).await?;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{}", answer.body);
        assert_eq!(answer.code(), "not_found");
    }
    // So does a known path asked with the wrong method, answered alike.
    let answer = call_with(&state, None, Method::GET, LOGIN, None).await?;
    assert_eq!(answer.status, StatusCode::NOT_FOUND, "{}", answer.body);
    assert_eq!(answer.code(), "not_found");
    let answer = call_with(&state, None, Method::GET, "/api/v1/auth/nothing", None).await?;
    assert_eq!(
        answer.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        answer.body
    );
    db.drop().await
}
