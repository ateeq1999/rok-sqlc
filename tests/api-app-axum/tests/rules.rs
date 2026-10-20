//! `tests/api-suite/rules.rs` against the axum app: the same answers as
//! `tests/api-app`'s.

extern crate rok_sqlc_api_app_axum as rok_sqlc_api_app;

mod common;

#[path = "../../api-suite/rules.rs"]
mod rules;
