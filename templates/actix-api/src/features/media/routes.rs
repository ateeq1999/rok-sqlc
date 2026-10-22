// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `media` routes, under `/api/v1/media`.

use actix_web::web;

use super::handlers;
use crate::shared::validation::{path_config, query_config};

/// Mounts every `media` endpoint under `/api/v1/media`. `POST` reads a
/// raw body of at most `max_upload_bytes` (`MEDIA_MAX_UPLOAD_BYTES`), its
/// own `PayloadConfig` in place of actix-web's default limit.
pub fn routes(cfg: &mut web::ServiceConfig, max_upload_bytes: usize) {
    cfg.service(
        web::scope("/api/v1/media")
            .app_data(path_config())
            .app_data(query_config())
            .service(
                web::resource("")
                    .app_data(web::PayloadConfig::new(max_upload_bytes))
                    .route(web::get().to(handlers::list))
                    .route(web::post().to(handlers::upload)),
            )
            .route("/{id}", web::get().to(handlers::metadata))
            .route("/{id}", web::delete().to(handlers::remove))
            .route("/{id}/content", web::get().to(handlers::content)),
    );
}
