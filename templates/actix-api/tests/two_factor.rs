//! Two-factor sign-in: `POST /auth/2fa/setup` then `/enable` turn it on,
//! after which the right password answers `requires_totp` and a pending
//! token that `/verify` trades, with a code or a recovery code, for a
//! session; `/disable` turns it off with the current password.

mod support;

use std::time::{SystemTime, UNIX_EPOCH};

use actix_web::http::{Method, StatusCode};
use serde_json::{Value, json};
use support::{Answer, Database, PASSWORD, TestResult, call, mailed, register_verified};
use totp_rs::{Algorithm, Secret, TOTP};

const EMAIL: &str = "ada@example.com";
const LOGIN: &str = "/api/v1/auth/login";
const SETUP: &str = "/api/v1/auth/2fa/setup";
const ENABLE: &str = "/api/v1/auth/2fa/enable";
const DISABLE: &str = "/api/v1/auth/2fa/disable";
const VERIFY: &str = "/api/v1/auth/2fa/verify";

type Error = Box<dyn std::error::Error>;

/// The code of the base32 `secret` for the time step `offset` steps from
/// now, as an authenticator app computes it.
fn code(secret: &str, offset: i64) -> Result<String, Error> {
    let bytes = Secret::Encoded(secret.to_owned()).to_bytes()?;
    let totp = TOTP::new_unchecked(Algorithm::SHA1, 6, 0, 30, bytes, None, String::new());
    let step = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() / 30;
    Ok(totp.generate(step.saturating_add_signed(offset) * 30))
}

/// `body`'s string `field`.
fn text(body: &Value, field: &str) -> Result<String, Error> {
    body[field]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("no `{field}` in {body}").into())
}

/// A verified account with two-factor on: its session token, its base32
/// secret and its recovery codes. Enabled with the code of the current
/// step, which is then spent.
async fn enrolled(pool: &sqlx::PgPool) -> Result<(String, String, Vec<String>), Error> {
    let token = register_verified(pool, EMAIL, PASSWORD).await?;
    let answer = call(pool, Some(&token), Method::POST, SETUP, None).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let secret = text(&answer.body, "secret")?;
    let body = json!({"code": code(&secret, 0)?});
    let answer = call(pool, Some(&token), Method::POST, ENABLE, Some(body)).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let codes = answer.body["recovery_codes"]
        .as_array()
        .ok_or("no recovery codes")?
        .iter()
        .filter_map(|code| code.as_str().map(str::to_owned))
        .collect();
    Ok((token, secret, codes))
}

/// Signs in with the right password: the pending token it answers.
async fn pending(pool: &sqlx::PgPool) -> Result<String, Error> {
    let body = json!({"email": EMAIL, "password": PASSWORD});
    let answer = call(pool, None, Method::POST, LOGIN, Some(body)).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(answer.body["requires_totp"], true, "{}", answer.body);
    assert!(answer.body.get("access_token").is_none(), "{}", answer.body);
    text(&answer.body, "pending_token")
}

/// `POST /auth/2fa/verify` with `pending_token` and `code`.
async fn verify(pool: &sqlx::PgPool, pending_token: &str, code: &str) -> Result<Answer, Error> {
    let body = json!({"pending_token": pending_token, "code": code});
    call(pool, None, Method::POST, VERIFY, Some(body)).await
}

#[actix_web::test]
async fn setup_enable_then_sign_in_with_a_code() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let token = register_verified(&db.pool, EMAIL, PASSWORD).await?;
    let answer = call(&db.pool, Some(&token), Method::POST, SETUP, None).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let secret = text(&answer.body, "secret")?;
    assert_eq!(secret.len(), 32, "{secret}");
    let uri = text(&answer.body, "provisioning_uri")?;
    assert!(uri.starts_with("otpauth://totp/"), "{uri}");
    assert!(uri.contains(&format!("secret={secret}")), "{uri}");
    let qr = text(&answer.body, "qr_code_data_uri")?;
    assert!(qr.starts_with("data:image/svg+xml;base64,"), "{qr}");

    // Not enforced until enabled.
    let body = json!({"email": EMAIL, "password": PASSWORD});
    let answer = call(&db.pool, None, Method::POST, LOGIN, Some(body)).await?;
    assert!(answer.body["access_token"].is_string(), "{}", answer.body);

    let answer = call(
        &db.pool,
        Some(&token),
        Method::POST,
        ENABLE,
        Some(json!({"code": "000000"})),
    )
    .await?;
    if code(&secret, 0)? != "000000" {
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    }
    let body = json!({"code": code(&secret, 0)?});
    let answer = call(
        &db.pool,
        Some(&token),
        Method::POST,
        ENABLE,
        Some(body.clone()),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let codes = answer.body["recovery_codes"].as_array().ok_or("no codes")?;
    assert_eq!(codes.len(), 10, "{}", answer.body);
    let answer = call(&db.pool, Some(&token), Method::POST, ENABLE, Some(body)).await?;
    assert_eq!(
        answer.status,
        StatusCode::BAD_REQUEST,
        "already on: {}",
        answer.body
    );
    let notices = mailed(&db.pool).await?;
    assert!(
        notices
            .iter()
            .any(|mail| mail.to == EMAIL && mail.subject.contains("Two-factor")),
        "{notices:?}"
    );

    let me = call(&db.pool, Some(&token), Method::GET, "/api/v1/auth/me", None).await?;
    assert_eq!(me.body["totp_enabled"], true, "{}", me.body);

    let pending_token = pending(&db.pool).await?;
    let answer = verify(&db.pool, &pending_token, &code(&secret, 1)?).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let session = text(&answer.body, "access_token")?;
    assert_eq!(answer.body["token_type"], "Bearer");
    let me = call(
        &db.pool,
        Some(&session),
        Method::GET,
        "/api/v1/auth/me",
        None,
    )
    .await?;
    assert_eq!(me.status, StatusCode::OK, "{}", me.body);

    // The pending token is spent.
    let answer = verify(&db.pool, &pending_token, &code(&secret, 1)?).await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "invalid_token");
    db.drop().await
}

#[actix_web::test]
async fn setup_needs_a_session_and_enable_needs_a_setup() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let answer = call(&db.pool, None, Method::POST, SETUP, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    let token = register_verified(&db.pool, EMAIL, PASSWORD).await?;
    let answer = call(
        &db.pool,
        Some(&token),
        Method::POST,
        ENABLE,
        Some(json!({"code": "123456"})),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    db.drop().await
}

#[actix_web::test]
async fn the_same_code_is_never_accepted_twice() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let (_, secret, _) = enrolled(&db.pool).await?;
    // Enabling spent the current step's code.
    let pending_token = pending(&db.pool).await?;
    let answer = verify(&db.pool, &pending_token, &code(&secret, 0)?).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);

    let next = code(&secret, 1)?;
    let answer = verify(&db.pool, &pending_token, &next).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let pending_token = pending(&db.pool).await?;
    let answer = verify(&db.pool, &pending_token, &next).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    db.drop().await
}

#[actix_web::test]
async fn a_recovery_code_works_once() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let (_, _, codes) = enrolled(&db.pool).await?;
    let recovery = codes.first().ok_or("no code")?;
    let pending_token = pending(&db.pool).await?;
    let answer = verify(&db.pool, &pending_token, &recovery.to_lowercase()).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert!(answer.body["access_token"].is_string(), "{}", answer.body);

    let pending_token = pending(&db.pool).await?;
    let answer = verify(&db.pool, &pending_token, recovery).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    let answer = verify(&db.pool, &pending_token, &codes[1]).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    db.drop().await
}

#[actix_web::test]
async fn wrong_codes_lock_the_account() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let (_, secret, _) = enrolled(&db.pool).await?;
    let pending_token = pending(&db.pool).await?;
    let right = code(&secret, 1)?;
    let wrong = if right == "000000" {
        "111111"
    } else {
        "000000"
    };
    for _ in 0..5 {
        let answer = verify(&db.pool, &pending_token, wrong).await?;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    }
    let answer = verify(&db.pool, &pending_token, &right).await?;
    assert_eq!(
        answer.status,
        StatusCode::UNAUTHORIZED,
        "locked: {}",
        answer.body
    );
    let body = json!({"email": EMAIL, "password": PASSWORD});
    let answer = call(&db.pool, None, Method::POST, LOGIN, Some(body)).await?;
    assert_eq!(
        answer.status,
        StatusCode::UNAUTHORIZED,
        "locked: {}",
        answer.body
    );
    db.drop().await
}

#[actix_web::test]
async fn disable_needs_the_current_password() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let (token, _, _) = enrolled(&db.pool).await?;
    mailed(&db.pool).await?;
    let body = json!({"current_password": "not the password"});
    let answer = call(&db.pool, Some(&token), Method::POST, DISABLE, Some(body)).await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "wrong_password");
    pending(&db.pool).await?;

    let body = json!({"current_password": PASSWORD});
    let answer = call(&db.pool, Some(&token), Method::POST, DISABLE, Some(body)).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let notices = mailed(&db.pool).await?;
    assert!(
        notices
            .iter()
            .any(|mail| mail.to == EMAIL && mail.subject.contains("Two-factor")),
        "{notices:?}"
    );
    let body = json!({"email": EMAIL, "password": PASSWORD});
    let answer = call(&db.pool, None, Method::POST, LOGIN, Some(body)).await?;
    assert!(answer.body["access_token"].is_string(), "{}", answer.body);
    let (secret, codes): (Option<String>, i64) =
        sqlx::query_as("SELECT totp_secret, (SELECT count(*) FROM totp_recovery_codes) FROM users")
            .fetch_one(&db.pool)
            .await?;
    assert_eq!((secret, codes), (None, 0));
    db.drop().await
}

#[actix_web::test]
async fn an_expired_or_unknown_pending_token_is_invalid() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let (_, secret, _) = enrolled(&db.pool).await?;
    let pending_token = pending(&db.pool).await?;
    sqlx::query("UPDATE auth_tokens SET expires_at = now() - interval '1 second' WHERE purpose = 'two_factor_pending'")
        .execute(&db.pool)
        .await?;
    let answer = verify(&db.pool, &pending_token, &code(&secret, 1)?).await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "invalid_token");
    let answer = verify(&db.pool, "made up", &code(&secret, 1)?).await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "invalid_token");
    db.drop().await
}

#[actix_web::test]
async fn a_pending_token_lasts_five_minutes() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    enrolled(&db.pool).await?;
    pending(&db.pool).await?;
    let (seconds,): (f64,) = sqlx::query_as(
        "SELECT extract(epoch FROM expires_at - now())::float8 FROM auth_tokens WHERE purpose = 'two_factor_pending'",
    )
    .fetch_one(&db.pool)
    .await?;
    // `expires_at` comes from the server's clock, `now()` from the database's.
    assert!((290.0..=301.0).contains(&seconds), "{seconds}");
    db.drop().await
}

#[actix_web::test]
async fn a_sign_in_without_two_factor_is_unchanged() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    register_verified(&db.pool, EMAIL, PASSWORD).await?;
    let body = json!({"email": EMAIL, "password": PASSWORD});
    let answer = call(&db.pool, None, Method::POST, LOGIN, Some(body)).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert!(answer.body["access_token"].is_string(), "{}", answer.body);
    assert!(
        answer.body.get("requires_totp").is_none(),
        "{}",
        answer.body
    );
    db.drop().await
}

#[actix_web::test]
async fn the_secret_is_encrypted_in_the_database() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let (_, secret, codes) = enrolled(&db.pool).await?;
    let (stored,): (Option<String>,) = sqlx::query_as("SELECT totp_secret FROM users")
        .fetch_one(&db.pool)
        .await?;
    let stored = stored.ok_or("no secret stored")?;
    assert!(!stored.contains(&secret), "{stored}");
    let raw = Secret::Encoded(secret).to_bytes()?;
    use base64::Engine;
    let sealed = base64::engine::general_purpose::STANDARD.decode(&stored)?;
    assert_eq!(sealed.len(), 12 + raw.len() + 16, "nonce, ciphertext, tag");
    assert!(
        !sealed
            .windows(raw.len())
            .any(|window| window == raw.as_slice())
    );
    let hashes: Vec<(String,)> = sqlx::query_as("SELECT code_hash FROM totp_recovery_codes")
        .fetch_all(&db.pool)
        .await?;
    assert_eq!(hashes.len(), 10);
    assert!(
        hashes
            .iter()
            .all(|(hash,)| !codes.contains(hash) && hash.len() == 64)
    );
    // Keyed: not the plain SHA-256 of any code.
    for code in &codes {
        let plain = actix_api::shared::security::token::hash(code);
        assert!(hashes.iter().all(|(hash,)| *hash != plain), "{code}");
    }
    db.drop().await
}

#[actix_web::test]
async fn disabling_two_factor_signs_every_session_out() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let (token, _, _) = enrolled(&db.pool).await?;
    let me = "/api/v1/auth/me";
    let answer = call(&db.pool, Some(&token), Method::GET, me, None).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let body = json!({"current_password": PASSWORD});
    let answer = call(&db.pool, Some(&token), Method::POST, DISABLE, Some(body)).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let answer = call(&db.pool, Some(&token), Method::GET, me, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    db.drop().await
}
