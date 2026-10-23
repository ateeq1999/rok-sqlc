// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `media` feature: each signed-in user's uploaded files, under
//! `/api/v1/media`. Hand-written in the generated features' layout;
//! `check-arch` applies to it in full. The bytes go through the `Storage`
//! port (`shared/storage`); its SQL is `queries/media.sql`
//! (`db::queries::media`).

/// Request and response bodies.
pub mod dto;
/// The handlers: parse, call one service method, answer.
pub mod handlers;
/// The URL to handler mapping.
pub mod routes;
/// `MediaService`: the uploads' rules.
pub mod service;

pub use routes::routes;
pub use service::MediaService;
