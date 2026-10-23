// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `account` routes: `/api/v1/auth/me`, `/api/v1/users/me`, its
//! `/avatar`, and the public `/api/v1/avatars/{file}`.

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, put};

use super::dto::AVATAR_MAX_BYTES;
use super::handlers;
use crate::app::AppState;
use crate::shared::rate_limit::RateLimit;

/// The signed-in user's own routes. `/api/v1/auth/me` counts against the
/// `/auth` rate `limit` like the rest of `/auth`. axum's router prefers a
/// static segment to a capture, so the generated `users` feature's
/// `/api/v1/users/{id}` never takes `/api/v1/users/me`, whatever the
/// order `app.rs` merges them in. `PUT /api/v1/users/me/avatar` reads a
/// raw body of at most 2 MiB, its own `DefaultBodyLimit`.
pub fn routes(limit: &RateLimit) -> Router<AppState> {
    let auth_me = Router::new()
        .route("/api/v1/auth/me", get(handlers::auth_me))
        .layer(limit.clone());
    Router::new()
        .route(
            "/api/v1/users/me",
            get(handlers::get_me).patch(handlers::update_me),
        )
        .route(
            "/api/v1/users/me/avatar",
            put(handlers::upload_avatar)
                .delete(handlers::remove_avatar)
                .layer(DefaultBodyLimit::max(AVATAR_MAX_BYTES)),
        )
        .route("/api/v1/avatars/{file}", get(handlers::avatar))
        .merge(auth_me)
}
