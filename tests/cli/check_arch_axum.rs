//! `rok-sqlc check-arch` knows the project's framework (0.7, decision A7):
//! rule 1 names the crate `[api] framework` picks, and the other framework's
//! crate is a rule-1 violation everywhere but the data layer, whose
//! `extractors` may serve both.

use super::check_arch::{assert_violations, check_arch, project, stdout};
use super::*;

/// The config of a project `init --framework axum` scaffolded.
const AXUM_CONFIG: &str = "[api]\noutput = \"src/features\"\nframework = \"axum\"\n";

/// A feature that follows every rule under axum: what `rok-sqlc g`
/// generates, in short, with the wiring and shared tools naming axum too.
const AXUM_NOTES: &[(&str, &str)] = &[
    (
        "src/features/notes/mod.rs",
        "pub mod handlers;\npub mod routes;\npub mod service;\n",
    ),
    (
        "src/features/notes/routes.rs",
        "use axum::Router;\npub fn routes() -> Router { Router::new() }\n",
    ),
    (
        "src/features/notes/handlers.rs",
        "use axum::response::Response;\npub async fn list() -> Option<Response> { None }\n",
    ),
    (
        "src/features/notes/service.rs",
        "use sqlx::PgPool;\npub struct NotesService { pub pool: PgPool }\n",
    ),
    ("src/features/generated.rs", "use axum::extract::FromRef;\n"),
    ("src/shared/validation.rs", "use axum::http::HeaderMap;\n"),
    ("src/app.rs", "use axum::Router;\n"),
    ("src/main.rs", "use axum::serve;\n"),
    ("src/error.rs", "use axum::response::IntoResponse;\n"),
    // `extractors` may name both frameworks: the data layer serves each.
    (
        "src/db/http.rs",
        "pub mod actix { use actix_web::FromRequest; }\npub mod axum { use axum::extract::FromRequest; }\n",
    ),
];

/// A scratch axum project holding `files`.
fn axum_project(files: &[(&str, &str)]) -> Result<TempDir> {
    let dir = project(files)?;
    fs::write(dir.path().join("rok-sqlc.toml"), AXUM_CONFIG)?;
    Ok(dir)
}

#[test]
fn an_axum_project_that_follows_the_rules_passes() -> Result<()> {
    let dir = axum_project(AXUM_NOTES)?;
    let output = check_arch(&dir, &[]);
    assert_success(&output);
    assert_eq!(stdout(&output), "");
    Ok(())
}

#[test]
fn rule_1_axum_outside_routes_and_handlers_is_reported() -> Result<()> {
    let dir = axum_project(AXUM_NOTES)?;
    fs::write(
        dir.path().join("src/features/notes/dto.rs"),
        "use axum::Json;\npub fn f(_: Json<()>) {}\n",
    )?;
    assert_violations(
        &check_arch(&dir, &[]),
        &[
            "src/features/notes/dto.rs:1: rule-1: `axum::Json`: only routes.rs, handlers.rs, app.rs, main.rs, error.rs, features/generated.rs, shared/ and db/ may use axum",
        ],
    );
    Ok(())
}

#[test]
fn rule_1_actix_web_in_an_axum_project_is_reported() -> Result<()> {
    let dir = axum_project(AXUM_NOTES)?;
    fs::write(
        dir.path().join("src/features/notes/handlers.rs"),
        "pub async fn list() -> actix_web::HttpResponse { todo_free() }\n",
    )?;
    fs::write(
        dir.path().join("src/shared/validation.rs"),
        "use actix_web::web;\n",
    )?;
    assert_violations(
        &check_arch(&dir, &[]),
        &[
            "src/features/notes/handlers.rs:1: rule-1: `actix_web::HttpResponse`: the project uses axum (`[api] framework`), so nothing but db/ may use actix_web",
            "src/shared/validation.rs:1: rule-1: `actix_web::web`: the project uses axum (`[api] framework`), so nothing but db/ may use actix_web",
        ],
    );
    Ok(())
}

#[test]
fn rule_1_axum_in_an_actix_project_is_reported() -> Result<()> {
    let dir = project(&[
        ("src/features/notes/mod.rs", "pub mod handlers;\n"),
        (
            "src/features/notes/handlers.rs",
            "use actix_web::HttpResponse;\nuse axum::Json;\n",
        ),
        ("src/db/http.rs", "pub mod axum { use axum::Json; }\n"),
    ])?;
    assert_violations(
        &check_arch(&dir, &[]),
        &[
            "src/features/notes/handlers.rs:2: rule-1: `axum::Json`: the project uses actix (`[api] framework`), so nothing but db/ may use axum",
        ],
    );
    Ok(())
}

/// `init --framework axum`'s `AGENTS.md` states rule 1 for axum, and the
/// scaffolded project passes `check-arch`.
#[test]
fn init_framework_axum_states_rule_1_for_axum() -> Result<()> {
    let dir = TempDir::new()?;
    assert_success(&run_binary_in(dir.path(), &["init", "--framework", "axum"]));
    let agents = fs::read_to_string(dir.path().join("AGENTS.md"))?;
    let agents = agents.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        agents.contains("1. In a feature, only `routes.rs` and `handlers.rs` use `axum`."),
        "{agents}"
    );
    assert!(!agents.contains("actix"), "{agents}");
    assert_success(&run_binary_in(dir.path(), &["check-arch"]));
    Ok(())
}
