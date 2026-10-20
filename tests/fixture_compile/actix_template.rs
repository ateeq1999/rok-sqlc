//! `templates/actix-api`, made the way a user makes it: `rok-sqlc init
//! --template actix` into a scratch directory, then `rok-sqlc g` there. The
//! generated files the template checks in must be exactly what `g` writes
//! (set `ROK_SQLC_UPDATE_EXPECTED=1` to rewrite them), and the project must
//! pass its own checks: rustfmt, the lint gate, `check-arch`, and `cargo
//! test` (its contract test, and its API tests against
//! `ROK_SQLC_TEST_DATABASE_URL` when set).

use anyhow::Result;

use crate::template_project::Template;

const ACTIX: Template = Template {
    name: "actix",
    dir: "actix-api",
};

#[test]
fn actix_template_checks_in_what_generation_writes() -> Result<()> {
    ACTIX.checks_in_what_generation_writes()
}

#[test]
fn actix_template_readme_lists_its_manifest() -> Result<()> {
    ACTIX.readme_lists_its_manifest()
}

#[test]
fn actix_template_passes_its_checks_and_tests() -> Result<()> {
    ACTIX.passes_its_checks_and_tests()
}
