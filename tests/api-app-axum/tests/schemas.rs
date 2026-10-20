//! `tests/api-suite/schemas.rs` against the axum app: the same answers as
//! `tests/api-app`'s.

extern crate rok_sqlc_api_app_axum as rok_sqlc_api_app;

#[path = "../../api-suite/schemas.rs"]
mod schemas;
