// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The account flows' cryptography, with no business meaning: password
//! hashing and its policy ([`password`]), single-use tokens ([`token`]) and
//! the session JWT ([`jwt`]). `shared/auth.rs` checks a caller with them.

pub mod jwt;
pub mod password;
pub mod token;
