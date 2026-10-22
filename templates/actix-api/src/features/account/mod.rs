// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `account` feature: the signed-in user's own account,
//! `GET /api/v1/auth/me` and `GET`/`PATCH /api/v1/users/me`, and its
//! profile photo: `PUT`/`DELETE /api/v1/users/me/avatar`, served publicly
//! at `GET /api/v1/avatars/{file}`. Hand-written in the generated
//! features' layout; `check-arch` applies to it in full. Its SQL is
//! `queries/account.sql` (`db::queries::account`).

/// Request and response bodies.
pub mod dto;
/// The handlers: parse, call one service method, answer.
pub mod handlers;
/// The URL to handler mapping.
pub mod routes;
/// `AccountService`: the account's rules.
pub mod service;

pub use routes::routes;
pub use service::AccountService;
