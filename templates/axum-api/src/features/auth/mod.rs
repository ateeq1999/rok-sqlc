// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `auth` feature, under `/api/v1/auth`: signing up and in, and the
//! flows behind mailed links. Hand-written in the generated features'
//! layout; `check-arch` applies to it in full. Its SQL is
//! `queries/auth.sql` (`db::queries::auth`).

/// Request and response bodies.
pub mod dto;
/// The handlers: parse, call one service method, answer.
pub mod handlers;
/// The URL to handler mapping.
pub mod routes;
/// `AuthService`: the flows' rules.
pub mod service;

pub use routes::routes;
pub use service::AuthService;
