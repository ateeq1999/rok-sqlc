//! `[api] framework` (0.7, decision A1): `actix` by default, or `axum`. The
//! framework's extractor must be in `extractors`, and an unknown framework
//! fails naming the known ones.

use super::api::{api_project, stderr};
use super::*;

/// A project like [`api_project`]'s, with `extractors` replaced by
/// `extractors` and `[api]` holding `framework` when given.
fn project(extractors: &str, framework: Option<&str>) -> Result<TempDir> {
    let dir = api_project("")?;
    let config = dir.path().join("rok-sqlc.toml");
    let text = fs::read_to_string(&config)?.replace(
        "extractors = [\"actix_web\"]",
        &format!("extractors = {extractors}"),
    );
    let framework = framework
        .map(|name| format!("framework = \"{name}\"\n"))
        .unwrap_or_default();
    fs::write(
        &config,
        format!("{text}[api]\noutput = \"src/features\"\n{framework}"),
    )?;
    Ok(dir)
}

#[test]
fn axum_generates_axum_routes() -> Result<()> {
    let dir = project("[\"axum\"]", Some("axum"))?;

    assert_success(&run_binary_in(dir.path(), &["g"]));

    let routes = fs::read_to_string(dir.path().join("src/features/users/routes.rs"))?;
    assert!(routes.contains("use axum::Router;"), "{routes}");
    assert!(!routes.contains("actix_web"), "{routes}");
    Ok(())
}

#[test]
fn actix_is_the_default_and_can_be_named() -> Result<()> {
    for framework in [None, Some("actix")] {
        let dir = project("[\"actix_web\"]", framework)?;

        assert_success(&run_binary_in(dir.path(), &["g"]));

        let routes = fs::read_to_string(dir.path().join("src/features/users/routes.rs"))?;
        assert!(routes.contains("use actix_web::web;"), "{routes}");
    }
    Ok(())
}

#[test]
fn an_unknown_framework_lists_the_known_ones() -> Result<()> {
    let dir = project("[\"axum\"]", Some("rocket"))?;

    let output = run_binary_in(dir.path(), &["g"]);

    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(stderr.contains("expected `actix` or `axum`"), "{stderr}");
    Ok(())
}

/// Each framework needs its own extractor: the API takes validated bodies
/// through it.
#[test]
fn the_framework_needs_its_extractor() -> Result<()> {
    let cases = [
        (
            "[\"actix_web\"]",
            Some("axum"),
            "`api.output` with `framework = \"axum\"` needs `derive = [\"serde\", \"validate\"]` and `axum` in `extractors`",
        ),
        (
            "[\"axum\"]",
            None,
            "`api.output` needs `derive = [\"serde\", \"validate\"]` and `actix_web` in `extractors`",
        ),
    ];
    for (extractors, framework, expected) in cases {
        let dir = project(extractors, framework)?;

        let output = run_binary_in(dir.path(), &["g"]);

        assert!(!output.status.success(), "{extractors} {framework:?}");
        let stderr = stderr(&output);
        assert!(stderr.contains(expected), "{stderr}");
    }
    // Both extractors are fine.
    let dir = project("[\"actix_web\", \"axum\"]", Some("axum"))?;
    assert_success(&run_binary_in(dir.path(), &["g"]));
    Ok(())
}
