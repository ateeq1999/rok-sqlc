//! `templates/axum-api`, made the way a user makes it: `rok-sqlc init
//! --template axum` into a scratch directory, then `rok-sqlc g` there. The
//! generated files the template checks in must be exactly what `g` writes
//! (set `ROK_SQLC_UPDATE_EXPECTED=1` to rewrite them), and the project must
//! pass its own checks: rustfmt, the lint gate, `check-arch`, and `cargo
//! test` (its contract test, and its API tests against
//! `ROK_SQLC_TEST_DATABASE_URL` when set).

use anyhow::Result;

use crate::template_project::Template;

const AXUM: Template = Template {
    name: "axum",
    dir: "axum-api",
};

#[test]
fn axum_template_checks_in_what_generation_writes() -> Result<()> {
    AXUM.checks_in_what_generation_writes()
}

#[test]
fn axum_template_readme_lists_its_manifest() -> Result<()> {
    AXUM.readme_lists_its_manifest()
}

#[test]
fn axum_template_passes_its_checks_and_tests() -> Result<()> {
    AXUM.passes_its_checks_and_tests()
}
