// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `auth` routes, under `/api/v1/auth`, behind the per-IP rate limit.

use actix_web::web;

use super::handlers;
use crate::shared::rate_limit::RateLimit;

/// Mounts every `auth` endpoint under `/api/v1/auth`, each request counted
/// against `limit`. `app.rs` mounts the `account` feature's
/// `/api/v1/auth/me` first, so this scope doesn't swallow it.
pub fn routes(cfg: &mut web::ServiceConfig, limit: &RateLimit) {
    cfg.service(
        web::scope("/api/v1/auth")
            .wrap(limit.clone())
            .route("/register", web::post().to(handlers::register))
            .route("/login", web::post().to(handlers::login))
            .route("/email/verify", web::post().to(handlers::verify))
            .route(
                "/email/verification/resend",
                web::post().to(handlers::resend),
            )
            .route("/password/forgot", web::post().to(handlers::forgot))
            .route("/password/reset", web::post().to(handlers::reset))
            .route("/password/change", web::post().to(handlers::change)),
    );
}
