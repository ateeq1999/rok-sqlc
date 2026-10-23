// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `two_factor` feature, under `/api/v1/auth/2fa`: setting two-factor
//! sign-in up, turning it on and off, and a sign-in's second step.
//! Hand-written in the generated features' layout; `check-arch` applies to
//! it in full. Its SQL is `queries/two_factor.sql`
//! (`db::queries::two_factor`).

/// Request and response bodies.
pub mod dto;
/// The handlers: parse, call one service method, answer.
pub mod handlers;
/// The URL to handler mapping.
pub mod routes;
/// `TwoFactorService`: the flows' rules.
pub mod service;

pub use routes::routes;
pub use service::TwoFactorService;
