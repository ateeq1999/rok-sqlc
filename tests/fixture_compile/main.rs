//! Generates the fixture schemas into the fixture crates and checks the
//! output the way a consuming crate would see it: `tests/fixture-app` (the
//! data layer), `tests/api-app` (the API features) and a project made with
//! `rok-sqlc init` and `rok-sqlc g`.
//!
//! Set `ROK_SQLC_TEST_DATABASE_URL` to a throwaway PostgreSQL database to
//! also execute the generated code (see `tests/fixture-app/tests/postgres/`).

#[path = "../common/mod.rs"]
mod common;

mod actix_template;
mod api_app;
mod fixture_app;
mod init;
