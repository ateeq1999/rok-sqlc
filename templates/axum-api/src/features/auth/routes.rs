// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `auth` routes, under `/api/v1/auth`, behind the per-IP rate limit.

use axum::Router;
use axum::routing::post;

use super::handlers;
use crate::app::AppState;
use crate::shared::rate_limit::RateLimit;

/// Every `auth` endpoint under `/api/v1/auth`, each request counted
/// against `limit`. The `account` feature's `/api/v1/auth/me` is its own
/// route, wrapped in the same `limit` by `account::routes`.
pub fn routes(limit: &RateLimit) -> Router<AppState> {
    Router::new()
        .route("/api/v1/auth/register", post(handlers::register))
        .route("/api/v1/auth/login", post(handlers::login))
        .route("/api/v1/auth/email/verify", post(handlers::verify))
        .route(
            "/api/v1/auth/email/verification/resend",
            post(handlers::resend),
        )
        .route("/api/v1/auth/password/forgot", post(handlers::forgot))
        .route("/api/v1/auth/password/reset", post(handlers::reset))
        .route("/api/v1/auth/password/change", post(handlers::change))
        .route("/api/v1/auth/email/change", post(handlers::request_change))
        .route(
            "/api/v1/auth/email/change/confirm",
            post(handlers::confirm_change),
        )
        .route("/api/v1/auth/invitations", post(handlers::invite))
        .layer(limit.clone())
}
