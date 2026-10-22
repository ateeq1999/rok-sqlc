// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `account` routes: `/api/v1/auth/me`, `/api/v1/users/me`, its
//! `/avatar`, and the public `/api/v1/avatars/{file}`.

use actix_web::web;

use super::dto::AVATAR_MAX_BYTES;
use super::handlers;
use crate::shared::rate_limit::RateLimit;

/// Mounts the signed-in user's own routes. `app.rs` mounts them before the
/// `auth` scope and the generated `users` feature, so neither
/// `/api/v1/auth` nor `/api/v1/users/{id}` captures them;
/// `/api/v1/auth/me` counts against the `/auth` rate `limit` like the rest
/// of `/auth`. `PUT /api/v1/users/me/avatar` reads a raw body of at most
/// 2 MiB, its own `PayloadConfig`.
pub fn routes(cfg: &mut web::ServiceConfig, limit: &RateLimit) {
    cfg.service(
        web::resource("/api/v1/auth/me")
            .wrap(limit.clone())
            .route(web::get().to(handlers::auth_me)),
    )
    .service(
        web::resource("/api/v1/users/me")
            .route(web::get().to(handlers::get_me))
            .route(web::patch().to(handlers::update_me)),
    )
    .service(
        web::resource("/api/v1/users/me/avatar")
            .app_data(web::PayloadConfig::new(AVATAR_MAX_BYTES))
            .route(web::put().to(handlers::upload_avatar))
            .route(web::delete().to(handlers::remove_avatar)),
    )
    .service(web::resource("/api/v1/avatars/{file}").route(web::get().to(handlers::avatar)));
}
