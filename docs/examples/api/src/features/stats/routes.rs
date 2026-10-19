//! The `stats` routes, under `/api/v1/stats`.

use actix_web::web;

use super::handlers;

/// Mounts `GET /api/v1/stats`.
pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("/api/v1/stats").route("", web::get().to(handlers::summary)));
}
