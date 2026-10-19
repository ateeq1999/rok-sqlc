//! The `stats` feature, written by hand: no table behind it, so rok-sqlc
//! generates none of it. It uses the same files as a generated feature,
//! is declared in `features/mod.rs`, and is mounted in `app.rs`.

/// The response body.
pub mod dto;
/// The handler: parse, call one service method, answer.
pub mod handlers;
/// The URL to handler mapping.
pub mod routes;
/// `StatsService`.
pub mod service;

pub use routes::routes;
pub use service::StatsService;
