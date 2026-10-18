//! An application laid out the way `rok-sqlc init` scaffolds one (ADR 0016),
//! compiled with the generated data layer, shared tools and API features.
//!
//! `tests/fixture_compile/` in rok-sqlc writes everything but this file and
//! `features/drafts/service.rs` (a scaffolded service the fixture changed):
//! the skeleton comes from `rok-sqlc init`, the rest from `rok-sqlc g`, and
//! none of it is checked in. This file stands in for the scaffolded
//! `lib.rs`: it declares the same modules, so the scaffolded `main.rs` and
//! the integration tests build the application, under the lints below. They
//! are the generated-code quality bar (ADR 0013), with the ones the
//! generated actix-web code passes too.

#![deny(missing_docs, warnings)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// The selected pedantic lints. They warn, so `deny(warnings)` and
// `cargo clippy -- -D warnings` turn any hit into an error.
#![warn(
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::explicit_iter_loop,
    clippy::implicit_clone,
    clippy::items_after_statements,
    clippy::manual_let_else,
    clippy::match_same_arms,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::redundant_closure_for_method_calls,
    clippy::ref_option,
    clippy::return_self_not_must_use,
    clippy::semicolon_if_nothing_returned,
    clippy::single_match_else,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::unnecessary_wraps
)]

pub mod app;
pub mod config;
pub mod db;
pub mod error;
pub mod features;
pub mod shared;
