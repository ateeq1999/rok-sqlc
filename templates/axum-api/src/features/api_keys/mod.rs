// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `api_keys` feature: a user's own API keys, `GET`/`POST
//! /api/v1/api-keys` and `DELETE /api/v1/api-keys/{id}`. Hand-written in
//! the generated features' layout; `check-arch` applies to it in full. Its
//! SQL is `queries/api_keys.sql` (`db::queries::api_keys`); a presented key
//! is checked by `shared/auth.rs`.

/// Request and response bodies.
pub mod dto;
/// The handlers: parse, call one service method, answer.
pub mod handlers;
/// The URL to handler mapping.
pub mod routes;
/// `ApiKeyService`: the keys' rules.
pub mod service;

pub use routes::routes;
pub use service::ApiKeyService;
