// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Tools with no business meaning, shared by the features. `shared/` never
//! imports `features/`.
//!
//! `rok-sqlc g` regenerates `health`, `pagination`, `response`, `tenant`
//! and `validation`; don't edit them. `auth`, `body`, `security`, `mail`,
//! `rate_limit`, `storage` and `thumbnail` are yours, like any tool you add
//! here (a queue); the ones with more than one implementation sit behind
//! a trait.

pub mod auth;
pub mod body;
pub mod health;
pub mod mail;
pub mod pagination;
pub mod rate_limit;
pub mod response;
pub mod security;
pub mod storage;
pub mod tenant;
pub mod thumbnail;
pub mod validation;
