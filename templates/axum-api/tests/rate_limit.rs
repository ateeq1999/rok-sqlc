//! `RateLimit`, the per-IP limit layer `/auth` is wrapped in: past the
//! limit it answers `429 too_many_requests` with `Retry-After`, per client
//! IP (the peer, or with `TRUST_PROXY` the last `X-Forwarded-For` address),
//! shared by every clone, and off at `0`. No database needed.

use std::net::SocketAddr;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use axum::routing::get;
use axum_api::shared::rate_limit::RateLimit;
use serde_json::Value;
use tower::ServiceExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A router answering `GET /` with `ok`, wrapped in `limit`.
fn limited(limit: &RateLimit) -> Router {
    Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(limit.clone())
}

/// `GET /` on `router` from the peer `peer` (none: no connection info),
/// with `X-Forwarded-For: forwarded` when given: its status, its
/// `Retry-After`, and its body.
async fn get_from(
    router: &Router,
    peer: Option<&str>,
    forwarded: Option<&str>,
) -> Result<(StatusCode, Option<String>, Vec<u8>), Box<dyn std::error::Error>> {
    let mut request = Request::get("/");
    if let Some(forwarded) = forwarded {
        request = request.header("x-forwarded-for", forwarded);
    }
    let mut request = request.body(Body::empty())?;
    if let Some(peer) = peer {
        let addr: SocketAddr = peer.parse()?;
        request.extensions_mut().insert(ConnectInfo(addr));
    }
    let response = router.clone().oneshot(request).await?;
    let status = response.status();
    let retry_after = response
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = to_bytes(response.into_body(), usize::MAX).await?.to_vec();
    Ok((status, retry_after, body))
}

const ALICE: Option<&str> = Some("10.0.0.1:5000");
const BOB: Option<&str> = Some("10.0.0.2:5000");

#[tokio::test]
async fn past_the_limit_a_client_is_answered_429_with_retry_after() -> TestResult {
    let router = limited(&RateLimit::per_minute(2, false));
    for _ in 0..2 {
        let (status, _, body) = get_from(&router, ALICE, None).await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, b"ok");
    }
    let (status, retry_after, body) = get_from(&router, ALICE, None).await?;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    let wait: u64 = retry_after.ok_or("no Retry-After")?.parse()?;
    assert!((1..=30).contains(&wait), "Retry-After: {wait}");
    let body: Value = serde_json::from_slice(&body)?;
    assert_eq!(body["error"]["code"], "too_many_requests");
    Ok(())
}

#[tokio::test]
async fn each_peer_has_its_own_limit_and_every_clone_shares_them() -> TestResult {
    let limit = RateLimit::per_minute(1, false);
    let (first, second) = (limited(&limit), limited(&limit));
    assert_eq!(get_from(&first, ALICE, None).await?.0, StatusCode::OK);
    assert_eq!(get_from(&first, BOB, None).await?.0, StatusCode::OK);
    let (status, _, _) = get_from(&second, ALICE, None).await?;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    Ok(())
}

#[tokio::test]
async fn x_forwarded_for_counts_only_behind_a_trusted_proxy() -> TestResult {
    let untrusted = limited(&RateLimit::per_minute(1, false));
    assert_eq!(
        get_from(&untrusted, ALICE, Some("203.0.113.7")).await?.0,
        StatusCode::OK
    );
    let (status, _, _) = get_from(&untrusted, ALICE, Some("203.0.113.8")).await?;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "the header was trusted"
    );

    let trusted = limited(&RateLimit::per_minute(1, true));
    for forwarded in ["203.0.113.7", "198.51.100.1, 203.0.113.8"] {
        let (status, _, _) = get_from(&trusted, ALICE, Some(forwarded)).await?;
        assert_eq!(status, StatusCode::OK, "{forwarded}");
    }
    let (status, _, _) = get_from(&trusted, BOB, Some("192.0.2.1, 203.0.113.8")).await?;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "the last address counts"
    );
    Ok(())
}

#[tokio::test]
async fn a_limit_of_zero_is_off() -> TestResult {
    let router = limited(&RateLimit::per_minute(0, false));
    for _ in 0..50 {
        assert_eq!(get_from(&router, None, None).await?.0, StatusCode::OK);
    }
    Ok(())
}
