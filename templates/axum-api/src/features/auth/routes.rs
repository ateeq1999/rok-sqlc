// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `auth` routes, under `/api/v1/auth`, behind the per-IP rate limit.

use axum::Router;
use axum::routing::post;

use super::handlers;
use crate::app::AppState;
use crate::error::AppError;
use crate::shared::rate_limit::RateLimit;

/// Every `auth` endpoint, nested under `/api/v1/auth`, with every request
/// under it counted against `limit`, as in actix-web's `/api/v1/auth`
/// scope: an unknown path there, or a known one asked with the wrong
/// method, answers the standard `404` and counts too. The `account`
/// feature's `/api/v1/auth/me` is its own route, wrapped in the same
/// `limit` by `account::routes`.
pub fn routes(limit: &RateLimit) -> Router<AppState> {
    let auth = Router::new()
        .route("/register", post(handlers::register))
        .route("/login", post(handlers::login))
        .route("/email/verify", post(handlers::verify))
        .route("/email/verification/resend", post(handlers::resend))
        .route("/password/forgot", post(handlers::forgot))
        .route("/password/reset", post(handlers::reset))
        .route("/password/change", post(handlers::change))
        .route("/email/change", post(handlers::request_change))
        .route("/email/change/confirm", post(handlers::confirm_change))
        .route("/invitations", post(handlers::invite))
        .route("/2fa/setup", post(handlers::setup_totp))
        .route("/2fa/enable", post(handlers::enable_totp))
        .route("/2fa/disable", post(handlers::disable_totp))
        .route("/2fa/verify", post(handlers::verify_totp))
        .fallback(no_such_route)
        .method_not_allowed_fallback(no_such_route)
        .layer(limit.clone());
    Router::new().nest("/api/v1/auth", auth)
}

/// An unknown path or method under `/api/v1/auth`: the wire standard's
/// `404`.
async fn no_such_route() -> AppError {
    AppError::NotFound("no such route".to_owned())
}
