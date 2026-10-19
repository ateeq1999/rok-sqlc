//! What the API tests share: the whole application over a PostgreSQL schema
//! of its own, requests to it, and callers' accounts and tokens.
//!
//! The tests need `DATABASE_URL`; without it they print a note and pass.
//! Each one creates a fresh schema holding the tables (`db::schema`'s
//! snapshot of `migrations/`) and drops it when done, so tests run in
//! parallel without seeing each other's rows.

#![allow(dead_code, reason = "each test file uses some of the helpers")]

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use actix_api::app::{AppState, configure};
use actix_api::db::schema::SCHEMA_SQL;
use actix_api::db::types::Role;
use actix_api::db::users::models::NewUser;
use actix_api::db::users::write;
use actix_api::shared::mail::{Mail, MemoryMailer, Outbox, Sender, Templates};
use actix_api::shared::security::jwt::JwtKeys;
use actix_web::http::{Method, StatusCode, header};
use actix_web::{App, test, web};
use serde_json::Value;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, PgPool};
use uuid::Uuid;

/// What a test returns.
pub type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The JWT secret the test application verifies tokens with.
const SECRET: &str = "test secret";

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
/// given, to the whole application over `pool`.
pub async fn call(
    pool: &PgPool,
    token: Option<&str>,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> Result<Answer, Box<dyn std::error::Error>> {
    let state = state(pool);
    let app = test::init_service(App::new().configure(|cfg| configure(cfg, &state))).await;
    let mut request = test::TestRequest::default().method(method).uri(uri);
    if let Some(token) = token {
        request = request.insert_header((header::AUTHORIZATION, format!("Bearer {token}")));
    }
    if let Some(body) = body {
        request = request.set_json(body);
    }
    let response = test::call_service(&app, request.to_request()).await;
    let status = response.status();
    let bytes = test::read_body(response).await;
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)?
    };
    Ok(Answer { status, body })
}

/// The keys the test application signs and verifies tokens with.
pub fn keys() -> web::Data<JwtKeys> {
    web::Data::new(JwtKeys::new(SECRET, Duration::from_secs(3600)))
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

/// The application's state over `pool`, as the tests configure it.
pub fn state(pool: &PgPool) -> AppState {
    AppState {
        pool: pool.clone(),
        keys: keys(),
        outbox: web::Data::new(Outbox::new()),
        templates: web::Data::new(Templates::new(APP_URL)),
    }
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
