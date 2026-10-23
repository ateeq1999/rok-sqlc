// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `two_factor` routes, under `/api/v1/auth/2fa`, behind the `/auth`
//! rate limit.

use axum::Router;
use axum::routing::post;

use super::handlers;
use crate::app::AppState;
use crate::shared::rate_limit::RateLimit;

/// Every `two_factor` endpoint, under `/api/v1/auth/2fa`, each request
/// counted against `limit` like the rest of `/auth`. Its routes are more
/// specific than the `auth` router nested at `/api/v1/auth`, so they win.
pub fn routes(limit: &RateLimit) -> Router<AppState> {
    Router::new()
        .route("/api/v1/auth/2fa/setup", post(handlers::setup_totp))
        .route("/api/v1/auth/2fa/enable", post(handlers::enable_totp))
        .route("/api/v1/auth/2fa/disable", post(handlers::disable_totp))
        .route("/api/v1/auth/2fa/verify", post(handlers::verify_totp))
        .layer(limit.clone())
}
