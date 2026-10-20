//! `app::time_out`, the request timeout `app::router` applies: a request
//! still running past it is answered `408 request_timeout` with the wire
//! standard's error body, like every other error; one within it is
//! answered as usual. No database needed.

use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use axum::middleware::from_fn_with_state;
use axum::routing::get;
use axum_api::app::time_out;
use serde_json::Value;
use tower::ServiceExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// `/slow`, which takes a minute, and `/fast`, behind a 50 ms timeout.
fn timed() -> Router {
    Router::new()
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                "late"
            }),
        )
        .route("/fast", get(|| async { "ok" }))
        .layer(from_fn_with_state(Duration::from_millis(50), time_out))
}

/// `GET path` on [`timed`]: its status and its body.
async fn get_path(path: &str) -> Result<(StatusCode, Vec<u8>), Box<dyn std::error::Error>> {
    let response = timed()
        .oneshot(Request::get(path).body(Body::empty())?)
        .await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await?.to_vec();
    Ok((status, body))
}

#[tokio::test]
async fn a_request_past_the_timeout_is_answered_408_with_the_error_body() -> TestResult {
    let (status, body) = get_path("/slow").await?;
    assert_eq!(status, StatusCode::REQUEST_TIMEOUT);
    let body: Value = serde_json::from_slice(&body)?;
    assert_eq!(body["error"]["code"], "request_timeout");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|m| !m.is_empty()),
        "{body}"
    );
    Ok(())
}

#[tokio::test]
async fn a_request_within_the_timeout_is_answered_as_usual() -> TestResult {
    let (status, body) = get_path("/fast").await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, b"ok");
    Ok(())
}
