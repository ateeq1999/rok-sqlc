//! The actix-web side of the API suites in `tests/api-suite/` (shared with
//! `tests/api-app-axum`): the framework's `Method`, `StatusCode` and test
//! attribute, and [`send`], which builds the whole app the scaffolded
//! `app.rs` configures and answers one request. The rest is
//! `tests/api-suite/client.rs`, re-exported.

#![allow(
    dead_code,
    unused_imports,
    reason = "each test file uses some of the helpers"
)]

#[path = "../../../api-suite/client.rs"]
mod client;

pub use actix_web::http::{Method, StatusCode};
pub use actix_web::test as api_test;
pub use client::*;

use actix_web::dev::Service;
use actix_web::http::header;
use actix_web::{App, HttpMessage, ResponseError, test as actix_test, web};
use rok_sqlc_api_app::app::configure;
use rok_sqlc_api_app::error::AppError;
use rok_sqlc_api_app::shared::auth::JwtKeys;
use sqlx::PgPool;

/// Sends `request` to the whole application over `pool`, verifying tokens
/// with `secret`. A tenant goes in the request's extensions through a
/// middleware, as the application's auth middleware would store it.
pub async fn send(pool: &PgPool, secret: &str, request: Request) -> Response {
    let keys = web::Data::new(JwtKeys::new(secret));
    let pool = pool.clone();
    let tenant = request.tenant;
    let app = App::new()
        .wrap_fn(move |req, srv| {
            if let Some(tenant) = tenant {
                req.extensions_mut().insert(tenant);
            }
            srv.call(req)
        })
        .configure(move |cfg| configure(cfg, &pool, &keys));
    let app = actix_test::init_service(app).await;
    let mut test_request = actix_test::TestRequest::default()
        .method(request.method)
        .uri(&request.uri);
    for (name, value) in request.headers {
        test_request = test_request.insert_header((name, value));
    }
    if let Some(body) = request.body {
        test_request = test_request.set_payload(body);
    }
    let response = actix_test::call_service(&app, test_request.to_request()).await;
    let header = |name| {
        response
            .headers()
            .get(name)
            .map(|value| value.to_str().expect("an ASCII header").to_owned())
    };
    Response {
        status: response.status(),
        location: header(header::LOCATION),
        etag: header(header::ETAG),
        content_type: header(header::CONTENT_TYPE),
        body: actix_test::read_body(response).await.to_vec(),
    }
}

/// The status `error` is answered with.
pub fn status_of(error: &AppError) -> StatusCode {
    error.status_code()
}
