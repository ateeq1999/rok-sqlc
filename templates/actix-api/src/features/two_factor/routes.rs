// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `two_factor` routes, under `/api/v1/auth/2fa`, behind the `/auth`
//! rate limit.

use actix_web::web;

use super::handlers;
use crate::shared::rate_limit::RateLimit;

/// Mounts every `two_factor` endpoint under `/api/v1/auth/2fa`, each
/// request counted against `limit` like the rest of `/auth`. `app.rs`
/// mounts it before the `auth` scope, so that scope doesn't swallow it.
pub fn routes(cfg: &mut web::ServiceConfig, limit: &RateLimit) {
    cfg.service(
        web::scope("/api/v1/auth/2fa")
            .wrap(limit.clone())
            .route("/setup", web::post().to(handlers::setup_totp))
            .route("/enable", web::post().to(handlers::enable_totp))
            .route("/disable", web::post().to(handlers::disable_totp))
            .route("/verify", web::post().to(handlers::verify_totp)),
    );
}
