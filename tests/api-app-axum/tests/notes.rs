//! `tests/api-suite/notes.rs` against the axum app: the same answers as
//! `tests/api-app`'s.

extern crate rok_sqlc_api_app_axum as rok_sqlc_api_app;

mod common;

#[path = "../../api-suite/notes.rs"]
mod notes;
