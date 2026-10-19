//! What the API tests share: the whole application over a PostgreSQL schema
//! of its own, requests to it, and callers' tokens.
//!
//! The tests need `DATABASE_URL`; without it they print a note and pass.
//! Each one creates a fresh schema holding the tables (`db::schema`'s
//! snapshot of `migrations/`) and drops it when done, so tests run in
//! parallel without seeing each other's rows.

#![allow(dead_code, reason = "each test file uses some of the helpers")]

use std::str::FromStr;

use actix_api::app::configure;
use actix_api::db::schema::SCHEMA_SQL;
use actix_api::shared::auth::{Claims, JwtKeys, Role};
use actix_web::http::{Method, StatusCode, header};
use actix_web::{App, test, web};
use serde_json::Value;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, PgPool};

/// What a test returns.
pub type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The JWT secret the test application verifies tokens with.
const SECRET: &str = "test secret";

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
    let keys = web::Data::new(JwtKeys::new(SECRET));
    let app = test::init_service(App::new().configure(|cfg| configure(cfg, pool, &keys))).await;
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

/// A bearer token for the caller `id` with `role`, valid for an hour.
pub fn token(id: uuid::Uuid, role: Role) -> Result<String, Box<dyn std::error::Error>> {
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        + 3600;
    let claims = Claims { sub: id, role, exp };
    Ok(jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(SECRET.as_bytes()),
    )?)
}

/// An admin's token.
pub fn admin() -> Result<String, Box<dyn std::error::Error>> {
    token(uuid::Uuid::now_v7(), Role::Admin)
}

/// A signed-in user's token, not an admin's.
pub fn user() -> Result<String, Box<dyn std::error::Error>> {
    token(uuid::Uuid::now_v7(), Role::User)
}
