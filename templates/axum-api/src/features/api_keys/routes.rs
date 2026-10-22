// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `api_keys` routes: `/api/v1/api-keys`.

use axum::Router;
use axum::routing::{delete, get};

use super::handlers;
use crate::app::AppState;

/// The signed-in user's own API keys. No key reaches them (see
/// `shared::security::api_key::SESSION_ONLY`): managing keys needs a
/// session.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/api-keys",
            get(handlers::list).post(handlers::create),
        )
        .route("/api/v1/api-keys/{id}", delete(handlers::revoke))
}
