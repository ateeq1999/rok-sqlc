//! A stand-in for the `features/generated.rs` that `rok-sqlc g` writes once
//! API features are generated (ticket 06), with the signatures the
//! scaffolded `app.rs` calls. Ticket 06 deletes it.

use actix_web::web;
use sqlx::PgPool;

/// Registers every feature's service as app data. No features yet.
pub fn register_services(_cfg: &mut web::ServiceConfig, _pool: &PgPool) {}

/// Mounts every feature's routes. No features yet.
pub fn configure_all(_cfg: &mut web::ServiceConfig) {}
