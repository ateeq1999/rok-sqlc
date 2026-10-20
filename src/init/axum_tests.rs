//! The axum skeleton's text (`init --framework axum`, 0.7 decision A2).
//! `tests/fixture_compile/init_axum.rs` compiles it with the generated
//! code and checks it end to end.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use tempfile::TempDir;

use super::*;
use crate::config::{Derive, Extractor};
use crate::config_file;

const CRATE: &str = "my_app";

fn skeleton() -> Vec<(PathBuf, String)> {
    plan(Path::new(""), CRATE, ApiFramework::Axum)
}

fn template(path: &str) -> String {
    skeleton()
        .into_iter()
        .find(|(planned, _)| planned == Path::new(path))
        .map(|(_, text)| text)
        .unwrap_or_else(|| panic!("no template for {path}"))
}

#[test]
fn the_axum_skeleton_has_the_actix_skeletons_files() {
    let paths = |framework| -> Vec<PathBuf> {
        plan(Path::new(""), CRATE, framework)
            .into_iter()
            .map(|(path, _)| path)
            .collect()
    };
    assert_eq!(paths(ApiFramework::Axum), paths(ApiFramework::Actix));
    for (path, text) in skeleton() {
        let first = text.lines().next().unwrap_or_default();
        assert!(
            first.contains("Scaffolded once by rok-sqlc"),
            "{}",
            path.display()
        );
        assert!(!text.contains('\r'), "{}", path.display());
        for placeholder in [CRATE_PLACEHOLDER, RULES_PLACEHOLDER, WIRING_PLACEHOLDER] {
            assert!(!text.contains(placeholder), "{}", path.display());
        }
    }
}

#[test]
fn axum_rust_templates_use_only_the_allowed_crates_and_never_panic() {
    let allowed = BTreeSet::from([
        "axum",
        "crate",
        "jsonwebtoken",
        "serde",
        "sqlx",
        "std",
        "thiserror",
        "tokio",
        "tower_http",
        "tracing",
        "tracing_subscriber",
        "utoipa",
        "utoipa_swagger_ui",
        "uuid",
        "validator",
        CRATE,
    ]);
    for (path, text) in skeleton() {
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        for line in text.lines() {
            if let Some(used) = line.trim().strip_prefix("use ") {
                let root = used.split("::").next().unwrap_or_default();
                assert!(allowed.contains(root), "{}: {line}", path.display());
            }
        }
        let text = text.replace("eprintln!", "");
        for banned in [
            ".unwrap()",
            ".expect(",
            "panic!",
            "todo!",
            "println!",
            "dbg!",
        ] {
            assert!(!text.contains(banned), "{} uses {banned}", path.display());
        }
        assert!(!text.contains("actix"), "{} names actix", path.display());
    }
}

#[test]
fn the_axum_config_file_turns_on_the_axum_api() -> Result<()> {
    let dir = TempDir::new()?;
    let file = dir.path().join("rok-sqlc.toml");
    fs::write(&file, template("rok-sqlc.toml"))?;
    let config = config_file::load(&file)?;
    assert_eq!(config.derive, Some(vec![Derive::Serde, Derive::Validate]));
    assert_eq!(config.extractors, Some(vec![Extractor::Axum]));
    let api = config.api.resolve().expect("the API is on");
    assert_eq!(api.framework, ApiFramework::Axum);
    let text = template("rok-sqlc.toml");
    for dependency in [
        "# axum = { version = \"0.8\", features = [\"macros\"] }",
        "# tokio = { version = \"1\", features = [\"rt-multi-thread\", \"macros\", \"signal\", \"net\", \"time\"] }",
        "# tower-http = { version = \"0.6\", features = [",
        "# utoipa-swagger-ui = { version = \"9\", features = [\"axum\", \"vendored\"] }",
    ] {
        assert!(text.contains(dependency), "missing {dependency}");
    }
    Ok(())
}

#[test]
fn the_axum_app_holds_the_state_and_serves_everything() {
    let text = template("src/app.rs");
    for wiring in [
        "#[derive(Clone, FromRef)]\npub struct AppState {",
        "pub services: Services,",
        "register_services(&pool)",
        ".merge(health::routes())",
        "SwaggerUi::new(\"/docs\").url(\"/api-docs/openapi.json\", document())",
        ".merge(routes())",
        "TraceLayer::new_for_http()",
        "CorsLayer::new()",
        "SetRequestIdLayer::x_request_id(MakeRequestUuid)",
        "middleware::from_fn_with_state(REQUEST_TIMEOUT, time_out)",
        "DefaultBodyLimit::max(BODY_LIMIT)",
        ".with_graceful_shutdown(shutdown_signal())",
    ] {
        assert!(text.contains(wiring), "missing {wiring}\n{text}");
    }
    assert!(template("src/main.rs").contains("#[tokio::main]"));
}

#[test]
fn the_axum_error_and_auth_answer_as_actix_does() {
    let error = template("src/error.rs");
    assert!(error.contains("impl IntoResponse for AppError"));
    assert!(error.contains("(status, Json(body)).into_response()"));
    let actix = super::plan(Path::new(""), CRATE, ApiFramework::Actix);
    let actix_error = &actix
        .iter()
        .find(|(path, _)| path == Path::new("src/error.rs"))
        .expect("an actix error.rs")
        .1;
    // The same variants, codes and statuses, but axum's request timeout
    // (`408 request_timeout`): actix-web has no timeout.
    let arms = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|line| line.trim_start().starts_with("AppError::"))
            .filter(|line| !line.contains("AppError::RequestTimeout"))
            .filter(|line| line.ends_with("\",") || line.contains("StatusCode::"))
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(arms(&error), arms(actix_error));
    assert!(error.contains("AppError::RequestTimeout(_) => \"request_timeout\","));
    assert!(error.contains("AppError::RequestTimeout(_) => StatusCode::REQUEST_TIMEOUT,"));
    let auth = template("src/shared/auth.rs");
    for piece in [
        "impl<S> FromRequestParts<S> for AuthUser",
        "impl<S> FromRequestParts<S> for AdminUser",
        "Arc<JwtKeys>: FromRef<S>,",
        "Algorithm::HS256",
        "strip_prefix(\"Bearer \")",
        "AppError::Forbidden(",
    ] {
        assert!(auth.contains(piece), "missing {piece}");
    }
    assert!(
        template("AGENTS.md")
            .contains("generated: declares the features, Services, register_services, routes")
    );
}
