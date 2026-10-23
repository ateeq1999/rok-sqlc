// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The account flows' cryptography, with no business meaning: password
//! hashing and its policy ([`password`]), the breached-password check
//! ([`breach`]), single-use tokens ([`token`]) and their lifecycle
//! ([`auth_token`]), and the session JWT ([`jwt`]). `shared/auth.rs`
//! checks a caller with them, and with [`api_key`] for a caller sending an
//! API key.

pub mod api_key;
pub mod auth_token;
pub mod breach;
pub mod jwt;
pub mod password;
pub mod token;
/// Two-factor sign-in: TOTP codes, the secret encrypted at rest, and the
/// recovery codes.
pub mod totp;
