//! `tests/api-suite/references.rs` against the axum app: the same answers as
//! `tests/api-app`'s.

extern crate rok_sqlc_api_app_axum as rok_sqlc_api_app;

mod common;

#[path = "../../api-suite/references.rs"]
mod references;
