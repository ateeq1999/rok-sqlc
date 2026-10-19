// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Tools with no business meaning, shared by the features. `shared/` never
//! imports `features/`.
//!
//! `rok-sqlc g` regenerates `health`, `pagination`, `response`, `tenant`
//! and `validation`; don't edit them. `auth`, `security` and `mail` are yours,
//! like any tool you add here (storage, a queue), each behind a trait.

pub mod auth;
pub mod health;
pub mod mail;
pub mod pagination;
pub mod response;
pub mod security;
pub mod tenant;
pub mod validation;
