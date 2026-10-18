//! The generated pool and `/health/*` routes, driven through the
//! application the scaffolded `app.rs` builds. `/health/ready` against a
//! reachable database runs only when `ROK_SQLC_TEST_DATABASE_URL` is set.

use std::time::Duration;

use actix_web::http::StatusCode;
use actix_web::{App, test as actix_test, web};
use rok_sqlc_api_app::app::configure;
use rok_sqlc_api_app::db::pool::{PoolSettings, check_health, init_pool};
use rok_sqlc_api_app::shared::auth::JwtKeys;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// A pool on a port nothing listens on, which fails within a second.
fn unreachable_pool() -> PgPool {
    PgPoolOptions::new()
        .acquire_timeout(Duration::from_secs(1))
        .connect_lazy("postgres://postgres:postgres@127.0.0.1:1/unreachable")
        .expect("a well-formed URL")
}

/// `GET path` on the whole application over `pool`: status and JSON body.
async fn get(pool: &PgPool, path: &str) -> (StatusCode, Value) {
    let keys = web::Data::new(JwtKeys::new("test secret"));
    let app =
        actix_test::init_service(App::new().configure(|cfg| configure(cfg, pool, &keys))).await;
    let response =
        actix_test::call_service(&app, actix_test::TestRequest::get().uri(path).to_request()).await;
    let status = response.status();
    let body = actix_test::read_body(response).await;
    (status, serde_json::from_slice(&body).expect("a JSON body"))
}

#[actix_web::test]
async fn live_answers_ok_without_the_database() {
    let (status, body) = get(&unreachable_pool(), "/health/live").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"status": "ok"}));
}

#[actix_web::test]
async fn ready_is_503_when_the_pool_cannot_connect() {
    let pool = unreachable_pool();
    assert!(check_health(&pool).await.is_err());
    let (status, body) = get(&pool, "/health/ready").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"]["code"], "service_unavailable", "{body}");
    assert!(body["error"]["message"].is_string(), "{body}");
    assert_eq!(
        body.as_object().map(serde_json::Map::len),
        Some(1),
        "{body}"
    );
}

#[actix_web::test]
async fn ready_answers_ready_against_postgres() {
    let Ok(url) = std::env::var("ROK_SQLC_TEST_DATABASE_URL") else {
        eprintln!("skipping: ROK_SQLC_TEST_DATABASE_URL is not set");
        return;
    };
    let settings = PoolSettings {
        min_connections: 1,
        ..PoolSettings::default()
    };
    let pool = init_pool(&url, &settings)
        .await
        .expect("connect to the test database");
    check_health(&pool).await.expect("the database answers");
    let (status, body) = get(&pool, "/health/ready").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"status": "ready"}));
}

#[actix_web::test]
async fn init_pool_fails_on_an_unreachable_database() {
    let settings = PoolSettings {
        acquire_timeout: Duration::from_secs(1),
        min_connections: 0,
        ..PoolSettings::default()
    };
    let url = "postgres://postgres:postgres@127.0.0.1:1/unreachable";
    assert!(init_pool(url, &settings).await.is_err());
}

#[test]
fn pool_settings_default_and_for_hardware() {
    let defaults = PoolSettings::default();
    assert_eq!(defaults.max_connections, 20);
    assert_eq!(defaults.min_connections, 5);
    assert_eq!(defaults.acquire_timeout, Duration::from_secs(3));
    assert_eq!(defaults.idle_timeout, Duration::from_secs(600));
    assert_eq!(defaults.max_lifetime, Duration::from_secs(1800));

    // 4 cores and 2 disks: 4 * 2 + 2.
    let sized = PoolSettings::for_hardware(4, 2);
    assert_eq!(sized.max_connections, 10);
    assert_eq!(sized.min_connections, 5);
    assert_eq!(sized.acquire_timeout, defaults.acquire_timeout);
    // A pool smaller than the default idle connections keeps no more open
    // than it holds.
    let small = PoolSettings::for_hardware(1, 1);
    assert_eq!(small.max_connections, 3);
    assert_eq!(small.min_connections, 3);
    assert_eq!(PoolSettings::for_hardware(0, 0).max_connections, 1);
}
