//! The application under test: every API test builds the whole app the
//! scaffolded `app.rs` configures, over a PostgreSQL schema of its own.
//!
//! The tests run only when `ROK_SQLC_TEST_DATABASE_URL` is set. Each one
//! creates a fresh schema holding the generated schema snapshot and drops it
//! when done, so tests run in parallel without seeing each other's rows.

#![allow(dead_code, reason = "each test file uses some of the helpers")]

use std::str::FromStr;

use actix_web::http::{Method, StatusCode, header};
use actix_web::web::Bytes;
use actix_web::{App, test as actix_test, web};
use rok_sqlc_api_app::app::configure;
use rok_sqlc_api_app::db::schema::SCHEMA_SQL;
use rok_sqlc_api_app::shared::auth::JwtKeys;
use serde_json::Value;
use sqlx::{AssertSqlSafe, PgPool};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

/// A schema of the test database holding a fresh copy of the tables, and a
/// pool whose connections use it.
pub struct Database {
    pub pool: PgPool,
    admin: PgPool,
    schema: String,
}

impl Database {
    /// A fresh schema, or `None` (and a note) when no database is set.
    pub async fn fresh() -> Option<Database> {
        let Ok(url) = std::env::var("ROK_SQLC_TEST_DATABASE_URL") else {
            eprintln!("skipping: ROK_SQLC_TEST_DATABASE_URL is not set");
            return None;
        };
        let schema = format!("api_app_{}", uuid::Uuid::now_v7().simple());
        let admin = PgPool::connect(&url)
            .await
            .expect("connect to the test database");
        sqlx::raw_sql(AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&admin)
            .await
            .expect("create the test schema");
        let options = PgConnectOptions::from_str(&url)
            .expect("a well-formed URL")
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .expect("connect to the test schema");
        sqlx::raw_sql(SCHEMA_SQL)
            .execute(&pool)
            .await
            .expect("apply the schema snapshot");
        Some(Database {
            pool,
            admin,
            schema,
        })
    }

    /// Drops the schema.
    pub async fn drop(self) {
        self.pool.close().await;
        sqlx::raw_sql(AssertSqlSafe(format!("DROP SCHEMA {} CASCADE", self.schema)))
            .execute(&self.admin)
            .await
            .expect("drop the test schema");
    }
}

/// One answer: its status, `Location` header, and body as JSON (`Null`
/// when empty).
#[derive(Debug)]
pub struct Answer {
    pub status: StatusCode,
    pub location: Option<String>,
    pub body: Value,
}

impl Answer {
    /// The wire standard's error code, `body.error.code`.
    pub fn code(&self) -> &str {
        self.body["error"]["code"].as_str().unwrap_or_default()
    }
}

/// Sends `request` to the whole application over `db`'s pool.
async fn send(
    db: &Database,
    request: actix_test::TestRequest,
) -> (StatusCode, Option<String>, Bytes) {
    let keys = web::Data::new(JwtKeys::new("test secret"));
    let pool = db.pool.clone();
    let app =
        actix_test::init_service(App::new().configure(move |cfg| configure(cfg, &pool, &keys)))
            .await;
    let response = actix_test::call_service(&app, request.to_request()).await;
    let status = response.status();
    let location = response
        .headers()
        .get(header::LOCATION)
        .map(|value| value.to_str().expect("an ASCII Location").to_owned());
    (status, location, actix_test::read_body(response).await)
}

/// Sends `method uri` with an optional JSON `body`.
pub async fn call(db: &Database, method: Method, uri: &str, body: Option<Value>) -> Answer {
    let mut request = actix_test::TestRequest::default().method(method).uri(uri);
    if let Some(body) = body {
        request = request.set_json(body);
    }
    let (status, location, bytes) = send(db, request).await;
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| panic!("a JSON body, got {}", String::from_utf8_lossy(&bytes)))
    };
    Answer {
        status,
        location,
        body,
    }
}

/// Sends `method uri` with `raw` as its JSON body, malformed or not.
pub async fn call_raw(db: &Database, method: Method, uri: &str, raw: &'static str) -> Answer {
    let request = actix_test::TestRequest::default()
        .method(method)
        .uri(uri)
        .insert_header((header::CONTENT_TYPE, "application/json"))
        .set_payload(raw);
    let (status, location, bytes) = send(db, request).await;
    Answer {
        status,
        location,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

/// `GET uri`.
pub async fn get(db: &Database, uri: &str) -> Answer {
    call(db, Method::GET, uri, None).await
}

/// `POST uri` with `body`, which must answer `201`; returns the body.
pub async fn create(db: &Database, uri: &str, body: Value) -> Value {
    let answer = call(db, Method::POST, uri, Some(body)).await;
    assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
    answer.body
}

/// Asserts `answer` is the wire standard's error body with `status` and
/// `code`: `{"error": {"code", "message", "details"?}}` and nothing else.
pub fn assert_error(answer: &Answer, status: StatusCode, code: &str) {
    assert_eq!(answer.status, status, "{answer:?}");
    assert_eq!(answer.code(), code, "{answer:?}");
    assert!(answer.body["error"]["message"].is_string(), "{answer:?}");
    assert_eq!(
        answer.body.as_object().map(serde_json::Map::len),
        Some(1),
        "{answer:?}"
    );
}
