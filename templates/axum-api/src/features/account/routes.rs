// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `account` routes: `/api/v1/auth/me` and `/api/v1/users/me`.

use axum::Router;
use axum::routing::get;

use super::handlers;
use crate::app::AppState;
use crate::shared::rate_limit::RateLimit;

/// The signed-in user's own routes. `/api/v1/auth/me` counts against the
/// `/auth` rate `limit` like the rest of `/auth`. axum's router prefers a
/// static segment to a capture, so the generated `users` feature's
/// `/api/v1/users/{id}` never takes `/api/v1/users/me`, whatever the
/// order `app.rs` merges them in.
pub fn routes(limit: &RateLimit) -> Router<AppState> {
    let auth_me = Router::new()
        .route("/api/v1/auth/me", get(handlers::auth_me))
        .layer(limit.clone());
    Router::new()
        .route(
            "/api/v1/users/me",
            get(handlers::get_me).patch(handlers::update_me),
        )
        .merge(auth_me)
}
