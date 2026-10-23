//! Passwords: `POST /auth/password/forgot` mails a reset link (and says
//! nothing about which emails exist), `POST /auth/password/reset` redeems
//! a reset or an invitation link, and `POST /auth/password/change` replaces
//! the signed-in user's password. Each signs every older session out.

mod support;

use std::sync::Arc;

use async_trait::async_trait;
use axum::http::{Method, StatusCode};
use axum_api::db::types::{AuthTokenPurpose, Role};
use axum_api::features::auth::service::issue_token;
use axum_api::shared::security::breach::BreachCheck;
use chrono::TimeDelta;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{
    Database, PASSWORD, TestResult, call, call_with, mailed, mailed_token, register_verified, state,
};

const FORGOT: &str = "/api/v1/auth/password/forgot";
const RESET: &str = "/api/v1/auth/password/reset";
const LOGIN: &str = "/api/v1/auth/login";
const CHANGE: &str = "/api/v1/auth/password/change";
const ME: &str = "/api/v1/auth/me";

/// A new password within the policy.
const NEW_PASSWORD: &str = "another long passphrase";

/// The one password [`KnowsOne`] knows from a breach.
const BREACHED: &str = "password123";

/// A breach check that knows one password.
struct KnowsOne;

#[async_trait]
impl BreachCheck for KnowsOne {
    async fn is_breached(&self, password: &str) -> bool {
        password == BREACHED
    }
}

fn credentials(email: &str, password: &str) -> Option<Value> {
    Some(json!({"email": email, "password": password}))
}

fn reset(token: &str, new_password: &str) -> Option<Value> {
    Some(json!({"token": token, "new_password": new_password}))
}

/// Asks for a reset link for `email` and answers the token it carries.
async fn forgot(pool: &PgPool, email: &str) -> Result<String, Box<dyn std::error::Error>> {
    let body = Some(json!({"email": email}));
    let answer = call(pool, None, Method::POST, FORGOT, body).await?;
    assert_eq!(answer.status, StatusCode::ACCEPTED, "{}", answer.body);
    let mail = mailed(pool).await?;
    let [mail] = mail.as_slice() else {
        return Err(format!("expected one mail, got {mail:?}").into());
    };
    assert_eq!(mail.to, email.trim().to_lowercase());
    Ok(mailed_token(mail).ok_or("no token in the reset mail")?)
}

#[tokio::test]
async fn forgot_for_an_unknown_email_is_accepted_and_mails_nothing() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let body = Some(json!({"email": "nobody@example.com"}));
    let answer = call(&db.pool, None, Method::POST, FORGOT, body).await?;
    assert_eq!(answer.status, StatusCode::ACCEPTED, "{}", answer.body);
    assert!(answer.body["message"].is_string(), "{}", answer.body);
    assert!(mailed(&db.pool).await?.is_empty());
    db.drop().await
}

#[tokio::test]
async fn a_reset_link_sets_the_password_once_and_signs_old_sessions_out() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let session = register_verified(&db.pool, "ada@example.com", PASSWORD).await?;
    let link = forgot(&db.pool, "Ada@Example.com ").await?;

    let answer = call(
        &db.pool,
        None,
        Method::POST,
        RESET,
        reset(&link, NEW_PASSWORD),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert!(answer.body["message"].is_string(), "{}", answer.body);

    let answer = call(&db.pool, Some(&session), Method::GET, ME, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    let answer = call(
        &db.pool,
        None,
        Method::POST,
        LOGIN,
        credentials("ada@example.com", PASSWORD),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    let answer = call(
        &db.pool,
        None,
        Method::POST,
        LOGIN,
        credentials("ada@example.com", NEW_PASSWORD),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);

    let answer = call(
        &db.pool,
        None,
        Method::POST,
        RESET,
        reset(&link, "a third passphrase"),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "invalid_token");
    db.drop().await
}

#[tokio::test]
async fn an_expired_reset_link_is_an_invalid_token() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    register_verified(&db.pool, "ada@example.com", PASSWORD).await?;
    let link = forgot(&db.pool, "ada@example.com").await?;
    sqlx::query("UPDATE auth_tokens SET expires_at = now() - interval '1 second'")
        .execute(&db.pool)
        .await?;
    let answer = call(
        &db.pool,
        None,
        Method::POST,
        RESET,
        reset(&link, NEW_PASSWORD),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "invalid_token");

    let answer = call(
        &db.pool,
        None,
        Method::POST,
        RESET,
        reset("not a token", NEW_PASSWORD),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "invalid_token");
    db.drop().await
}

#[tokio::test]
async fn a_newer_reset_link_replaces_the_older_one() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    register_verified(&db.pool, "ada@example.com", PASSWORD).await?;
    let older = forgot(&db.pool, "ada@example.com").await?;
    let newer = forgot(&db.pool, "ada@example.com").await?;
    let answer = call(
        &db.pool,
        None,
        Method::POST,
        RESET,
        reset(&older, NEW_PASSWORD),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    let answer = call(
        &db.pool,
        None,
        Method::POST,
        RESET,
        reset(&newer, NEW_PASSWORD),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    db.drop().await
}

#[tokio::test]
async fn a_reset_lifts_a_lockout() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    register_verified(&db.pool, "ada@example.com", PASSWORD).await?;
    for _ in 0..5 {
        let body = credentials("ada@example.com", "not the password");
        call(&db.pool, None, Method::POST, LOGIN, body).await?;
    }
    let link = forgot(&db.pool, "ada@example.com").await?;
    let answer = call(
        &db.pool,
        None,
        Method::POST,
        RESET,
        reset(&link, NEW_PASSWORD),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let answer = call(
        &db.pool,
        None,
        Method::POST,
        LOGIN,
        credentials("ada@example.com", NEW_PASSWORD),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    db.drop().await
}

#[tokio::test]
async fn an_invitation_link_sets_the_first_password_and_verifies_the_email() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let (id, _) = support::account(&db.pool, Role::User).await?;
    let mut conn = db.pool.acquire().await?;
    let link = issue_token(
        &mut conn,
        id.into(),
        AuthTokenPurpose::Invitation,
        TimeDelta::days(7),
        None,
    )
    .await?;
    drop(conn);
    let email = format!("caller-{}@test.invalid", id.simple());
    let answer = call(
        &db.pool,
        None,
        Method::POST,
        RESET,
        reset(&link.raw, NEW_PASSWORD),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let answer = call(
        &db.pool,
        None,
        Method::POST,
        LOGIN,
        credentials(&email, NEW_PASSWORD),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);

    let answer = call(
        &db.pool,
        None,
        Method::POST,
        RESET,
        reset(&link.raw, PASSWORD),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "invalid_token");
    db.drop().await
}

#[tokio::test]
async fn a_reset_refuses_a_weak_or_breached_password_and_keeps_the_link() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    register_verified(&db.pool, "ada@example.com", PASSWORD).await?;
    let link = forgot(&db.pool, "ada@example.com").await?;
    let state = state(&db.pool)?.with_breach(Arc::new(KnowsOne));

    let answer = call_with(&state, None, Method::POST, RESET, reset(&link, "short")).await?;
    assert_eq!(
        answer.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        answer.body
    );
    assert_eq!(answer.code(), "validation_failed");
    let answer = call_with(&state, None, Method::POST, RESET, reset(&link, BREACHED)).await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "breached_password");

    let answer = call_with(
        &state,
        None,
        Method::POST,
        RESET,
        reset(&link, NEW_PASSWORD),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    db.drop().await
}

#[tokio::test]
async fn a_bad_link_is_refused_before_the_new_password_is_checked() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let state = state(&db.pool)?.with_breach(Arc::new(KnowsOne));
    // A breached password with a bad link: the link is looked at first, so
    // the breach service is never asked and no hash is computed.
    let garbage = "x".repeat(43);
    let answer = call_with(&state, None, Method::POST, RESET, reset(&garbage, BREACHED)).await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "invalid_token");

    // Nor does a link issued for another purpose.
    let (id, _) = support::account(&db.pool, Role::User).await?;
    let mut conn = db.pool.acquire().await?;
    let other = issue_token(
        &mut conn,
        id.into(),
        AuthTokenPurpose::EmailVerification,
        TimeDelta::hours(1),
        None,
    )
    .await?;
    drop(conn);
    let answer = call_with(
        &state,
        None,
        Method::POST,
        RESET,
        reset(&other.raw, BREACHED),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "invalid_token");
    db.drop().await
}

fn change(current_password: &str, new_password: &str) -> Option<Value> {
    Some(json!({"current_password": current_password, "new_password": new_password}))
}

#[tokio::test]
async fn a_change_with_a_wrong_current_password_is_refused() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let session = register_verified(&db.pool, "ada@example.com", PASSWORD).await?;
    let body = change("not the password", NEW_PASSWORD);
    let answer = call(&db.pool, Some(&session), Method::POST, CHANGE, body).await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "wrong_password");

    let answer = call(&db.pool, Some(&session), Method::GET, ME, None).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let answer = call(
        &db.pool,
        None,
        Method::POST,
        CHANGE,
        change(PASSWORD, NEW_PASSWORD),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    db.drop().await
}

#[tokio::test]
async fn a_change_signs_every_session_out_and_mails_a_notice() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let session = register_verified(&db.pool, "ada@example.com", PASSWORD).await?;
    let body = change(PASSWORD, NEW_PASSWORD);
    let answer = call(&db.pool, Some(&session), Method::POST, CHANGE, body).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert!(answer.body["message"].is_string(), "{}", answer.body);

    let answer = call(&db.pool, Some(&session), Method::GET, ME, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    let answer = call(
        &db.pool,
        None,
        Method::POST,
        LOGIN,
        credentials("ada@example.com", PASSWORD),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    let answer = call(
        &db.pool,
        None,
        Method::POST,
        LOGIN,
        credentials("ada@example.com", NEW_PASSWORD),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);

    let mail = mailed(&db.pool).await?;
    let [notice] = mail.as_slice() else {
        return Err(format!("expected one notice, got {mail:?}").into());
    };
    assert_eq!(notice.to, "ada@example.com");
    assert_eq!(notice.subject, "Your password was changed");
    db.drop().await
}

#[tokio::test]
async fn a_change_refuses_a_weak_or_breached_password() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let session = register_verified(&db.pool, "ada@example.com", PASSWORD).await?;
    let state = state(&db.pool)?.with_breach(Arc::new(KnowsOne));
    let answer = call_with(
        &state,
        Some(&session),
        Method::POST,
        CHANGE,
        change(PASSWORD, "short"),
    )
    .await?;
    assert_eq!(
        answer.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        answer.body
    );
    let answer = call_with(
        &state,
        Some(&session),
        Method::POST,
        CHANGE,
        change(PASSWORD, BREACHED),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "breached_password");

    let answer = call_with(&state, Some(&session), Method::GET, ME, None).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    db.drop().await
}

/// A write API key for the caller of `session`: the raw key, checked to
/// work on `GET /users/me`.
async fn api_key(pool: &PgPool, session: &str) -> Result<String, Box<dyn std::error::Error>> {
    let body = Some(json!({"name": "ci", "scope": "write"}));
    let answer = call(pool, Some(session), Method::POST, "/api/v1/api-keys", body).await?;
    assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.body);
    let key = answer.body["key"].as_str().unwrap_or_default().to_owned();
    let answer = call(pool, Some(&key), Method::GET, USERS_ME, None).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    Ok(key)
}

const USERS_ME: &str = "/api/v1/users/me";

#[tokio::test]
async fn a_reset_revokes_api_keys_and_pending_two_factor_sign_ins() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let (id, session) = support::account(&db.pool, Role::User).await?;
    let key = api_key(&db.pool, &session).await?;
    let mut conn = db.pool.acquire().await?;
    let pending = issue_token(
        &mut conn,
        id.into(),
        AuthTokenPurpose::TwoFactorPending,
        TimeDelta::minutes(5),
        None,
    )
    .await?;
    let link = issue_token(
        &mut conn,
        id.into(),
        AuthTokenPurpose::PasswordReset,
        TimeDelta::hours(1),
        None,
    )
    .await?;
    drop(conn);

    let body = reset(&link.raw, NEW_PASSWORD);
    let answer = call(&db.pool, None, Method::POST, RESET, body).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);

    let answer = call(&db.pool, Some(&key), Method::GET, USERS_ME, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    let body = Some(json!({"pending_token": pending.raw, "code": "123456"}));
    let answer = call(
        &db.pool,
        None,
        Method::POST,
        "/api/v1/auth/2fa/verify",
        body,
    )
    .await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "invalid_token");
    db.drop().await
}

#[tokio::test]
async fn a_change_keeps_api_keys() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let session = register_verified(&db.pool, "ada@example.com", PASSWORD).await?;
    let key = api_key(&db.pool, &session).await?;
    let body = change(PASSWORD, NEW_PASSWORD);
    let answer = call(&db.pool, Some(&session), Method::POST, CHANGE, body).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let answer = call(&db.pool, Some(&key), Method::GET, USERS_ME, None).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    db.drop().await
}
