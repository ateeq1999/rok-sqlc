//! What the API tests share: the whole application over a PostgreSQL schema
//! of its own, requests to it, and callers' accounts and tokens.
//!
//! The tests need `DATABASE_URL`; without it they print a note and pass.
//! Each one creates a fresh schema holding the tables (`db::schema`'s
//! snapshot of `migrations/`) and drops it when done, so tests run in
//! parallel without seeing each other's rows.

#![allow(dead_code, reason = "each test file uses some of the helpers")]

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use axum_api::app::{AppState, router};
use axum_api::config::Config;
use axum_api::db::schema::SCHEMA_SQL;
use axum_api::db::types::Role;
use axum_api::db::users::models::NewUser;
use axum_api::db::users::write;
use axum_api::shared::mail::{Mail, MemoryMailer, Outbox, Sender};
use axum_api::shared::security::jwt::JwtKeys;
use serde_json::Value;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, PgPool};
use tower::ServiceExt;
use uuid::Uuid;

/// What a test returns.
pub type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The JWT secret the test application verifies tokens with.
const SECRET: &str = "a test secret of at least 32 bytes";

/// The `TOTP_ENCRYPTION_KEY` the test application encrypts two-factor
/// secrets with: base64 of 32 bytes.
pub const TOTP_KEY: &str = "dGVzdCBrZXkgb2YgZXhhY3RseSB0aGlydHktdHdvIGI=";

/// The `APP_URL` the test application's mailed links start with.
pub const APP_URL: &str = "http://app.test";

/// A schema of the test database, and a pool whose connections use it.
pub struct Database {
    pub pool: PgPool,
    admin: PgPool,
    schema: String,
}

impl Database {
    /// A fresh schema, or `None` (and a note) when `DATABASE_URL` is not set.
    pub async fn fresh() -> Result<Option<Database>, Box<dyn std::error::Error>> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL is not set");
            return Ok(None);
        };
        let schema = format!("test_{}", uuid::Uuid::now_v7().simple());
        let admin = PgPool::connect(&url).await?;
        sqlx::raw_sql(AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&admin)
            .await?;
        let options = PgConnectOptions::from_str(&url)?.options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        sqlx::raw_sql(SCHEMA_SQL).execute(&pool).await?;
        Ok(Some(Database {
            pool,
            admin,
            schema,
        }))
    }

    /// Drops the schema.
    pub async fn drop(self) -> TestResult {
        self.pool.close().await;
        sqlx::raw_sql(AssertSqlSafe(format!(
            "DROP SCHEMA {} CASCADE",
            self.schema
        )))
        .execute(&self.admin)
        .await?;
        Ok(())
    }
}

/// One answer: its status and its body as JSON (`Null` when empty).
#[derive(Debug)]
pub struct Answer {
    pub status: StatusCode,
    pub body: Value,
}

impl Answer {
    /// The wire standard's error code, `body.error.code`.
    pub fn code(&self) -> &str {
        self.body["error"]["code"].as_str().unwrap_or_default()
    }
}

/// Sends `method uri`, with the bearer `token` and the JSON `body` when
/// given, to the whole application over `pool`, built afresh from
/// [`state`] (so its rate limit starts empty).
pub async fn call(
    pool: &PgPool,
    token: Option<&str>,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> Result<Answer, Box<dyn std::error::Error>> {
    call_with(&state(pool)?, token, method, uri, body).await
}

/// Sends `method uri` like [`call`], to the application over `state`:
/// calls sharing a state share its rate limit.
pub async fn call_with(
    state: &AppState,
    token: Option<&str>,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> Result<Answer, Box<dyn std::error::Error>> {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body)?))?,
        None => request.body(Body::empty())?,
    };
    let response = router(state.clone()).oneshot(request).await?;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)?
    };
    Ok(Answer { status, body })
}

/// Sends `method uri` like [`call`], with `headers` (name, value) instead
/// of a bearer token: an `X-API-Key`, say.
pub async fn call_headers(
    pool: &PgPool,
    headers: &[(&str, &str)],
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> Result<Answer, Box<dyn std::error::Error>> {
    let mut request = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let request = match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body)?))?,
        None => request.body(Body::empty())?,
    };
    let response = router(state(pool)?).oneshot(request).await?;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)?
    };
    Ok(Answer { status, body })
}

/// The keys the test application signs and verifies tokens with.
pub fn keys() -> JwtKeys {
    JwtKeys::new(SECRET, Duration::from_secs(3600))
}

/// A bearer token for the account `id` with `role` and `token_version`,
/// valid for an hour.
pub fn token(
    id: Uuid,
    role: Role,
    token_version: i32,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(keys().issue(id, role, token_version)?)
}

/// A new account with `role`, and a token for it: what signing in gives.
pub async fn account(
    pool: &PgPool,
    role: Role,
) -> Result<(Uuid, String), Box<dyn std::error::Error>> {
    let id = Uuid::now_v7();
    let row = write::create(
        pool,
        NewUser {
            id: Some(id.into()),
            email: format!("caller-{}@test.invalid", id.simple()),
            display_name: None,
            role: Some(role),
            is_active: None,
        },
    )
    .await?;
    Ok((id, token(id, role, row.token_version)?))
}

/// A token of a new admin account.
pub async fn admin(pool: &PgPool) -> Result<String, Box<dyn std::error::Error>> {
    Ok(account(pool, Role::Admin).await?.1)
}

/// A token of a new account that is not an admin's.
pub async fn user(pool: &PgPool) -> Result<String, Box<dyn std::error::Error>> {
    Ok(account(pool, Role::User).await?.1)
}

/// The settings the test application runs with: [`SECRET`] for 1h tokens,
/// links under [`APP_URL`] and the defaults, with `overrides` (such as
/// `("REQUIRE_VERIFIED_EMAIL", "false")`) set on top.
pub fn config(overrides: &[(&str, &str)]) -> Result<Config, Box<dyn std::error::Error>> {
    let mut vars: HashMap<&str, &str> = HashMap::from([
        ("DATABASE_URL", "postgres://unused"),
        ("JWT_SECRET", SECRET),
        ("TOTP_ENCRYPTION_KEY", TOTP_KEY),
        ("JWT_TTL", "3600"),
        ("APP_URL", APP_URL),
    ]);
    vars.extend(overrides.iter().copied());
    Ok(Config::from_vars(|name| {
        vars.get(name).map(|value| (*value).to_owned())
    })?)
}

/// The application's state over `pool`, as the tests configure it.
pub fn state(pool: &PgPool) -> Result<AppState, Box<dyn std::error::Error>> {
    state_with(pool, &[])
}

/// The application's state over `pool`, with the settings `overrides` (see
/// [`config`]).
pub fn state_with(
    pool: &PgPool,
    overrides: &[(&str, &str)],
) -> Result<AppState, Box<dyn std::error::Error>> {
    Ok(AppState::new(pool.clone(), &config(overrides)?))
}

/// Delivers every due mail in `pool`'s outbox, as the sender would, and
/// returns what was sent, oldest first.
pub async fn mailed(pool: &PgPool) -> Result<Vec<Mail>, Box<dyn std::error::Error>> {
    let mailer = Arc::new(MemoryMailer::default());
    let sender = Sender::new(pool.clone(), mailer.clone(), &Outbox::new());
    while sender.deliver_due().await?.sent > 0 {}
    Ok(mailer.sent())
}

/// The token of the link `mail` carries (`…?token=<token>`), if any.
pub fn mailed_token(mail: &Mail) -> Option<String> {
    let start = mail.text.find("?token=")? + "?token=".len();
    let token: String = mail.text[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (!token.is_empty()).then_some(token)
}
/// A password the sign-up helpers use: within the policy, and not one a
/// breach check knows.
pub const PASSWORD: &str = "a long and unusual passphrase";

/// Signs `email` up with `password` (`POST /auth/register`), verifies it
/// with the mailed link, and signs in: the new account's bearer token. The
/// outbox is delivered (and its mail dropped) on the way.
pub async fn register_verified(
    pool: &PgPool,
    email: &str,
    password: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let credentials = serde_json::json!({"email": email, "password": password});
    let register = "/api/v1/auth/register";
    let answer = call(
        pool,
        None,
        Method::POST,
        register,
        Some(credentials.clone()),
    )
    .await?;
    if answer.status != StatusCode::CREATED {
        return Err(format!("register answered {}: {}", answer.status, answer.body).into());
    }
    let address = email.trim().to_lowercase();
    let link = mailed(pool)
        .await?
        .iter()
        .rev()
        .find(|mail| mail.to == address)
        .and_then(mailed_token)
        .ok_or("no verification mail")?;
    let body = serde_json::json!({"token": link});
    let answer = call(
        pool,
        None,
        Method::POST,
        "/api/v1/auth/email/verify",
        Some(body),
    )
    .await?;
    if answer.status != StatusCode::OK {
        return Err(format!("verify answered {}: {}", answer.status, answer.body).into());
    }
    let answer = call(
        pool,
        None,
        Method::POST,
        "/api/v1/auth/login",
        Some(credentials),
    )
    .await?;
    match answer.body["access_token"].as_str() {
        Some(token) => Ok(token.to_owned()),
        None => Err(format!("login answered {}: {}", answer.status, answer.body).into()),
    }
}

/// One answer as bytes: its status, its headers (names lowercase) and its
/// body, for routes that don't answer JSON.
#[derive(Debug)]
pub struct Raw {
    pub status: StatusCode,
    pub headers: HashMap<String, String>,
    pub bytes: Vec<u8>,
}

impl Raw {
    /// The body as JSON (`Null` when empty).
    pub fn json(&self) -> Result<Value, serde_json::Error> {
        if self.bytes.is_empty() {
            Ok(Value::Null)
        } else {
            serde_json::from_slice(&self.bytes)
        }
    }

    /// The header `name` (lowercase), if answered.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }
}

/// Sends `method uri` with the bearer `token`, the `headers` and the raw
/// `body` to the application over `state`, and answers bytes.
pub async fn send(
    state: &AppState,
    token: Option<&str>,
    method: Method,
    uri: &str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> Result<Raw, Box<dyn std::error::Error>> {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let request = request
        .header(header::CONTENT_LENGTH, body.len())
        .body(Body::from(body))?;
    let response = router(state.clone()).oneshot(request).await?;
    let status = response.status();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
        })
        .collect();
    let bytes = to_bytes(response.into_body(), usize::MAX).await?.to_vec();
    Ok(Raw {
        status,
        headers,
        bytes,
    })
}

/// A [`Storage`](axum_api::shared::storage::Storage) in memory that knows
/// which keys hold an object: swap it in with `AppState::with_storage`,
/// and read [`Recorded::keys`] to see what was left behind.
#[derive(Clone)]
pub struct Recorded {
    inner: axum_api::shared::storage::ObjectStorage,
    keys: Arc<std::sync::Mutex<std::collections::BTreeSet<String>>>,
}

impl Recorded {
    /// An empty store.
    pub fn new() -> Self {
        Recorded {
            inner: axum_api::shared::storage::ObjectStorage::memory(),
            keys: Arc::default(),
        }
    }

    /// The keys holding an object now.
    pub fn keys(&self) -> Vec<String> {
        self.keys
            .lock()
            .map(|keys| keys.iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[async_trait::async_trait]
impl axum_api::shared::storage::Storage for Recorded {
    async fn put(
        &self,
        key: &str,
        bytes: bytes::Bytes,
        content_type: &str,
    ) -> Result<(), axum_api::error::AppError> {
        self.inner.put(key, bytes, content_type).await?;
        if let Ok(mut keys) = self.keys.lock() {
            keys.insert(key.to_owned());
        }
        Ok(())
    }

    async fn get(
        &self,
        key: &str,
    ) -> Result<Option<axum_api::shared::storage::StoredObject>, axum_api::error::AppError> {
        self.inner.get(key).await
    }

    async fn delete(&self, key: &str) -> Result<(), axum_api::error::AppError> {
        self.inner.delete(key).await?;
        if let Ok(mut keys) = self.keys.lock() {
            keys.remove(key);
        }
        Ok(())
    }
}
