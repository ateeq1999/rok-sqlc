// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `api_keys` routes: `/api/v1/api-keys`.

use actix_web::web;

use super::handlers;

/// Mounts the signed-in user's own API keys. No key reaches them (see
/// `shared::security::api_key::SESSION_ONLY`): managing keys needs a
/// session.
pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/api/v1/api-keys")
            .route(web::get().to(handlers::list))
            .route(web::post().to(handlers::create)),
    )
    .service(web::resource("/api/v1/api-keys/{id}").route(web::delete().to(handlers::revoke)));
}
