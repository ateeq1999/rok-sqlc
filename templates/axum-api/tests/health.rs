//! `/health/live` and `/health/ready`, outside the API prefix.

mod support;

use axum::http::{Method, StatusCode};
use serde_json::json;
use support::{Database, TestResult, call};

#[tokio::test]
async fn the_process_is_live_and_ready_with_its_database() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let answer = call(&db.pool, None, Method::GET, "/health/live", None).await?;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.body, json!({"status": "ok"}));
    let answer = call(&db.pool, None, Method::GET, "/health/ready", None).await?;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.body, json!({"status": "ready"}));
    db.drop().await
}

/// No database needed: the pool points at a port nothing listens on.
#[tokio::test]
async fn the_process_is_not_ready_without_its_database() -> TestResult {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(2))
        .connect_lazy("postgres://postgres@127.0.0.1:1/unreachable")?;
    let answer = call(&pool, None, Method::GET, "/health/ready", None).await?;
    assert_eq!(answer.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(answer.code(), "service_unavailable");
    Ok(())
}
