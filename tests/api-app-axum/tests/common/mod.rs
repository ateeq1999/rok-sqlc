//! The axum side of the API suites in `tests/api-suite/` (shared with
//! `tests/api-app`): the framework's `Method`, `StatusCode` and test
//! attribute, and [`send`], which drives the router the scaffolded `app.rs`
//! builds with one request. The rest is `tests/api-suite/client.rs`,
//! re-exported.

#![allow(
    dead_code,
    unused_imports,
    reason = "each test file uses some of the helpers"
)]

#[path = "../../../api-suite/client.rs"]
mod client;

pub use axum::http::{Method, StatusCode};
pub use client::*;
pub use tokio::test as api_test;

use axum::body::Body;
use axum::http::header;
use rok_sqlc_api_app::app::{AppState, router};
use rok_sqlc_api_app::error::AppError;
use rok_sqlc_api_app::shared::auth::JwtKeys;
use sqlx::PgPool;
use tower::ServiceExt;

/// Sends `request` to the whole application over `pool`, verifying tokens
/// with `secret`. A tenant goes in the request's extensions, where the
/// application's auth middleware would store it.
pub async fn send(pool: &PgPool, secret: &str, request: Request) -> Response {
    let app = router(AppState::new(pool.clone(), JwtKeys::new(secret)));
    let mut builder = axum::http::Request::builder()
        .method(request.method)
        .uri(&request.uri);
    for (name, value) in request.headers {
        builder = builder.header(name, value);
    }
    let mut http_request = builder
        .body(request.body.map(Body::from).unwrap_or_else(Body::empty))
        .expect("a well-formed request");
    if let Some(tenant) = request.tenant {
        http_request.extensions_mut().insert(tenant);
    }
    let response = app
        .oneshot(http_request)
        .await
        .expect("an infallible router");
    let header = |name| {
        response
            .headers()
            .get(name)
            .map(|value: &axum::http::HeaderValue| {
                value.to_str().expect("an ASCII header").to_owned()
            })
    };
    let (status, location, etag, content_type) = (
        response.status(),
        header(header::LOCATION),
        header(header::ETAG),
        header(header::CONTENT_TYPE),
    );
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a readable body");
    Response {
        status,
        location,
        etag,
        content_type,
        body: body.to_vec(),
    }
}

/// The status `error` is answered with.
pub fn status_of(error: &AppError) -> StatusCode {
    error.status_code()
}
