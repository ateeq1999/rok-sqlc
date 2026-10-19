//! The application under test: every API test builds the whole app the
//! scaffolded `app.rs` configures, over a PostgreSQL schema of its own.
//!
//! The tests run only when `ROK_SQLC_TEST_DATABASE_URL` is set. Each one
//! creates a fresh schema holding the generated schema snapshot and drops it
//! when done, so tests run in parallel without seeing each other's rows.

#![allow(dead_code, reason = "each test file uses some of the helpers")]

use std::str::FromStr;

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode, header};
use actix_web::web::Bytes;
use actix_web::{App, HttpMessage, test as actix_test, web};
use rok_sqlc_api_app::app::configure;
use rok_sqlc_api_app::db::schema::SCHEMA_SQL;
use rok_sqlc_api_app::shared::auth::{Claims, JwtKeys, Role};
use serde_json::Value;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, PgPool};

/// The JWT secret the test application verifies tokens with.
const SECRET: &str = "test secret";

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
        sqlx::raw_sql(AssertSqlSafe(format!(
            "DROP SCHEMA {} CASCADE",
            self.schema
        )))
        .execute(&self.admin)
        .await
        .expect("drop the test schema");
    }
}

/// One answer: its status, `Location` and `ETag` headers, and body as JSON
/// (`Null` when empty).
#[derive(Debug)]
pub struct Answer {
    pub status: StatusCode,
    pub location: Option<String>,
    pub etag: Option<String>,
    pub body: Value,
}

impl Answer {
    /// The wire standard's error code, `body.error.code`.
    pub fn code(&self) -> &str {
        self.body["error"]["code"].as_str().unwrap_or_default()
    }
}

/// The headers of an answer the tests read.
struct Headers {
    location: Option<String>,
    etag: Option<String>,
}

/// Sends `request` to the whole application over `db`'s pool. With a
/// `tenant`, a middleware stores it in the request's extensions, as the
/// application's auth middleware would.
async fn send(
    db: &Database,
    request: actix_test::TestRequest,
    tenant: Option<uuid::Uuid>,
) -> (StatusCode, Headers, Bytes) {
    let keys = web::Data::new(JwtKeys::new(SECRET));
    let pool = db.pool.clone();
    let app = App::new()
        .wrap_fn(move |req, srv| {
            if let Some(tenant) = tenant {
                req.extensions_mut().insert(tenant);
            }
            srv.call(req)
        })
        .configure(move |cfg| configure(cfg, &pool, &keys));
    let app = actix_test::init_service(app).await;
    let response = actix_test::call_service(&app, request.to_request()).await;
    let status = response.status();
    let header = |name| {
        response
            .headers()
            .get(name)
            .map(|value| value.to_str().expect("an ASCII header").to_owned())
    };
    let headers = Headers {
        location: header(header::LOCATION),
        etag: header(header::ETAG),
    };
    (status, headers, actix_test::read_body(response).await)
}

/// What a request carries besides its method, URI and body: the caller's
/// bearer token, the tenant the auth middleware found, and `If-Match`.
#[derive(Debug, Clone, Default)]
pub struct With {
    pub token: Option<String>,
    pub tenant: Option<uuid::Uuid>,
    pub if_match: Option<String>,
}

/// Sends `method uri` with an optional JSON `body` and what `with` holds.
pub async fn call_with(
    db: &Database,
    with: &With,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> Answer {
    let mut request = actix_test::TestRequest::default().method(method).uri(uri);
    if let Some(token) = &with.token {
        request = request.insert_header((header::AUTHORIZATION, format!("Bearer {token}")));
    }
    if let Some(version) = &with.if_match {
        request = request.insert_header((header::IF_MATCH, version.as_str()));
    }
    if let Some(body) = body {
        request = request.set_json(body);
    }
    let (status, headers, bytes) = send(db, request, with.tenant).await;
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| panic!("a JSON body, got {}", String::from_utf8_lossy(&bytes)))
    };
    Answer {
        status,
        location: headers.location,
        etag: headers.etag,
        body,
    }
}

/// Sends `method uri` with an optional JSON `body`.
pub async fn call(db: &Database, method: Method, uri: &str, body: Option<Value>) -> Answer {
    call_as(db, None, method, uri, body).await
}

/// A bearer token for the caller `id` with `role`, signed with the secret
/// the test application verifies with, valid for an hour.
pub fn token(id: uuid::Uuid, role: Role) -> String {
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_secs()
        + 3600;
    let claims = Claims { sub: id, role, exp };
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(SECRET.as_bytes()),
    )
    .expect("an encodable token")
}

/// Sends `method uri` with an optional JSON `body`, as the caller whose
/// bearer `token` it is, or with no `Authorization` header.
pub async fn call_as(
    db: &Database,
    token: Option<&str>,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> Answer {
    let with = With {
        token: token.map(str::to_owned),
        ..With::default()
    };
    call_with(db, &with, method, uri, body).await
}

/// Sends `method uri` with `raw` as its JSON body, malformed or not.
pub async fn call_raw(db: &Database, method: Method, uri: &str, raw: &'static str) -> Answer {
    let request = actix_test::TestRequest::default()
        .method(method)
        .uri(uri)
        .insert_header((header::CONTENT_TYPE, "application/json"))
        .set_payload(raw);
    let (status, headers, bytes) = send(db, request, None).await;
    Answer {
        status,
        location: headers.location,
        etag: headers.etag,
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

/// `GET uri` against the whole application, over a pool that never
/// connects: the document and Swagger UI need no database. Returns the
/// status, the `Content-Type` and `Location` headers, and the body.
pub async fn fetch(uri: &str) -> (StatusCode, String, String, Vec<u8>) {
    let pool = PgPool::connect_lazy("postgres://localhost/unused").expect("a well-formed URL");
    let keys = web::Data::new(JwtKeys::new("unused"));
    let app = App::new().configure(move |cfg| configure(cfg, &pool, &keys));
    let app = actix_test::init_service(app).await;
    let request = actix_test::TestRequest::get().uri(uri).to_request();
    let response = actix_test::call_service(&app, request).await;
    let status = response.status();
    let header = |name| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    let content_type = header(header::CONTENT_TYPE);
    let location = header(header::LOCATION);
    let body = actix_test::read_body(response).await.to_vec();
    (status, content_type, location, body)
}

/// The served OpenAPI document, `/api-docs/openapi.json`.
pub async fn document() -> Value {
    let (status, content_type, _, body) = fetch("/api-docs/openapi.json").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        content_type.starts_with("application/json"),
        "{content_type}"
    );
    serde_json::from_slice(&body).expect("a JSON document")
}
