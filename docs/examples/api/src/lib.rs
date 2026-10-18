// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The application, as a library: `main.rs` starts it, and tests build it
//! with `app::configure`. Every module is public, so generated code the
//! application does not use yet is not dead code.

pub mod app;
pub mod config;
pub mod db;
pub mod error;
pub mod features;
pub mod openapi;
pub mod shared;
