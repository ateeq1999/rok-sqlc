//! Signing up: `POST /auth/register` creates an unverified account and
//! mails a verification link; `POST /auth/email/verify` redeems it once;
//! `POST /auth/email/verification/resend` mails a new one.

mod support;

use std::sync::Arc;

use actix_api::shared::security::breach::BreachCheck;
use actix_web::http::{Method, StatusCode};
use async_trait::async_trait;
use serde_json::{Value, json};
use support::{
    APP_URL, Database, PASSWORD, TestResult, call, call_with, mailed, mailed_token,
    register_verified, state, state_with,
};

const REGISTER: &str = "/api/v1/auth/register";
const VERIFY: &str = "/api/v1/auth/email/verify";
const RESEND: &str = "/api/v1/auth/email/verification/resend";
const LOGIN: &str = "/api/v1/auth/login";

fn credentials(email: &str, password: &str) -> Option<Value> {
    Some(json!({"email": email, "password": password}))
}

fn token(token: &str) -> Option<Value> {
    Some(json!({ "token": token }))
}

#[actix_web::test]
async fn register_creates_an_unverified_user_and_mails_a_verification_link() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let body = credentials("  Ada@Example.COM ", PASSWORD);
    let answer = call(&db.pool, None, Method::POST, REGISTER, body).await?;
    assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.body);
    assert_eq!(answer.body["email"], "ada@example.com");
    assert_eq!(answer.body["role"], "user");
    assert_eq!(answer.body["email_verified"], false);
    assert_eq!(answer.body["has_password"], true);
    assert!(
        answer.body.get("password_hash").is_none(),
        "{}",
        answer.body
    );

    let mail = mailed(&db.pool).await?;
    assert_eq!(mail.len(), 1, "{mail:?}");
    assert_eq!(mail[0].to, "ada@example.com");
    let link = format!("{APP_URL}/verify-email?token=");
    assert!(mail[0].text.contains(&link), "{}", mail[0].text);
    assert!(mailed_token(&mail[0]).is_some());
    db.drop().await
}

#[actix_web::test]
async fn the_mailed_link_verifies_the_email_once_and_then_sign_in_works() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let body = credentials("ada@example.com", PASSWORD);
    let answer = call(&db.pool, None, Method::POST, REGISTER, body).await?;
    assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.body);
    let link = mailed(&db.pool).await?;
    let link = link.first().and_then(mailed_token).ok_or("no link")?;

    let answer = call(&db.pool, None, Method::POST, VERIFY, token(&link)).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert!(answer.body["message"].is_string(), "{}", answer.body);

    let answer = call(&db.pool, None, Method::POST, VERIFY, token(&link)).await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "invalid_token");

    let body = credentials("ADA@example.com", PASSWORD);
    let answer = call(&db.pool, None, Method::POST, LOGIN, body).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(answer.body["token_type"], "Bearer");
    assert_eq!(answer.body["expires_in"], 3600);
    let session = answer.body["access_token"].as_str().ok_or("no session")?;
    let answer = call(
        &db.pool,
        Some(session),
        Method::GET,
        "/api/v1/auth/me",
        None,
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(answer.body["email_verified"], true);
    db.drop().await
}

#[actix_web::test]
async fn an_unknown_token_is_an_invalid_token() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let answer = call(&db.pool, None, Method::POST, VERIFY, token("nope")).await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "invalid_token");
    db.drop().await
}

#[actix_web::test]
async fn a_taken_email_whatever_its_case_is_a_conflict() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let body = credentials("ada@example.com", PASSWORD);
    let answer = call(&db.pool, None, Method::POST, REGISTER, body).await?;
    assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.body);
    let body = credentials("Ada@Example.com", PASSWORD);
    let answer = call(&db.pool, None, Method::POST, REGISTER, body).await?;
    assert_eq!(answer.status, StatusCode::CONFLICT, "{}", answer.body);
    assert_eq!(answer.code(), "conflict");
    db.drop().await
}

#[actix_web::test]
async fn a_short_or_long_password_or_a_bad_email_is_a_validation_failure() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let body = credentials("ada@example.com", "short");
    let answer = call(&db.pool, None, Method::POST, REGISTER, body).await?;
    assert_eq!(
        answer.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        answer.body
    );
    assert_eq!(answer.code(), "validation_failed");
    let details = &answer.body["error"]["details"];
    assert!(details["password"].is_array(), "{}", answer.body);

    let body = credentials("ada@example.com", &"x".repeat(129));
    let answer = call(&db.pool, None, Method::POST, REGISTER, body).await?;
    assert_eq!(
        answer.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        answer.body
    );

    let body = credentials("not-an-email", PASSWORD);
    let answer = call(&db.pool, None, Method::POST, REGISTER, body).await?;
    assert_eq!(
        answer.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        answer.body
    );
    assert!(
        answer.body["error"]["details"]["email"].is_array(),
        "{}",
        answer.body
    );
    assert!(mailed(&db.pool).await?.is_empty());
    db.drop().await
}

/// A breach check that knows one password.
struct KnowsOne;

#[async_trait]
impl BreachCheck for KnowsOne {
    async fn is_breached(&self, password: &str) -> bool {
        password == "password123"
    }
}

#[actix_web::test]
async fn a_breached_password_is_refused_and_makes_no_account() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let mut state = state(&db.pool)?;
    state.breach = Arc::new(KnowsOne);
    let body = credentials("ada@example.com", "password123");
    let answer = call_with(&state, None, Method::POST, REGISTER, body).await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "breached_password");

    let body = credentials("ada@example.com", PASSWORD);
    let answer = call_with(&state, None, Method::POST, REGISTER, body).await?;
    assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.body);
    db.drop().await
}

#[actix_web::test]
async fn resend_mails_a_new_link_and_spends_the_old_one() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let state = state_with(&db.pool, &[("REQUIRE_VERIFIED_EMAIL", "false")])?;
    let body = credentials("ada@example.com", PASSWORD);
    call_with(&state, None, Method::POST, REGISTER, body.clone()).await?;
    let first = mailed(&db.pool).await?;
    let first = first.first().and_then(mailed_token).ok_or("no link")?;
    let answer = call_with(&state, None, Method::POST, LOGIN, body).await?;
    let session = answer.body["access_token"].as_str().ok_or("no session")?;

    let answer = call_with(&state, Some(session), Method::POST, RESEND, None).await?;
    assert_eq!(answer.status, StatusCode::ACCEPTED, "{}", answer.body);
    let mail = mailed(&db.pool).await?;
    assert_eq!(mail.len(), 1, "{mail:?}");
    let second = mailed_token(&mail[0]).ok_or("no link")?;
    assert_ne!(first, second);

    let answer = call(&db.pool, None, Method::POST, VERIFY, token(&first)).await?;
    assert_eq!(answer.code(), "invalid_token", "{}", answer.body);
    let answer = call(&db.pool, None, Method::POST, VERIFY, token(&second)).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    db.drop().await
}

#[actix_web::test]
async fn resend_for_a_verified_email_mails_nothing() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let session = register_verified(&db.pool, "ada@example.com", PASSWORD).await?;
    let answer = call(&db.pool, Some(&session), Method::POST, RESEND, None).await?;
    assert_eq!(answer.status, StatusCode::ACCEPTED, "{}", answer.body);
    assert!(mailed(&db.pool).await?.is_empty());

    let answer = call(&db.pool, None, Method::POST, RESEND, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    db.drop().await
}
