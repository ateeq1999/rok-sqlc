//! `tests/api-app-axum`: `tests/api-app` for axum (0.7, decision A7),
//! generated from the same migrations and settings with `[api] framework =
//! "axum"`. Its suites are `tests/api-app`'s, from `tests/api-suite/`, so
//! the two apps answer the same.

use anyhow::Result;
use rok_sqlc::ApiFramework;

use crate::api_app::{is_rustfmt_clean, passes_check_arch, passes_its_tests, passes_the_lint_gate};

#[test]
fn api_app_axum_passes_the_lint_gate() -> Result<()> {
    passes_the_lint_gate(ApiFramework::Axum)
}

#[test]
fn api_app_axum_passes_its_tests() -> Result<()> {
    passes_its_tests(ApiFramework::Axum)
}

#[test]
fn api_app_axum_passes_check_arch() -> Result<()> {
    passes_check_arch(ApiFramework::Axum)
}

#[test]
fn api_app_axum_generated_code_is_rustfmt_clean() -> Result<()> {
    is_rustfmt_clean(ApiFramework::Axum)
}
