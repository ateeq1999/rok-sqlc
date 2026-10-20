//! The skeleton's text, template by template. `tests/fixture_compile/`
//! compiles it with the generated code and checks it end to end.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use tempfile::TempDir;

use super::*;
use crate::config::{Derive, Extractor, IdGeneration};
use crate::config_file;

/// The library crate the tests scaffold for.
const CRATE: &str = "my_app";

fn skeleton() -> Vec<(PathBuf, String)> {
    plan(Path::new(""), CRATE, ApiFramework::Actix)
}

fn template(path: &str) -> String {
    skeleton()
        .into_iter()
        .find(|(planned, _)| planned == Path::new(path))
        .map(|(_, text)| text)
        .unwrap_or_else(|| panic!("no template for {path}"))
}

fn rust_templates() -> Vec<(PathBuf, String)> {
    skeleton()
        .into_iter()
        .filter(|(path, _)| path.extension().is_some_and(|ext| ext == "rs"))
        .collect()
}

#[test]
fn every_template_starts_with_the_scaffold_header() {
    for (path, text) in skeleton() {
        let first = text.lines().next().unwrap_or_default();
        let words = SCAFFOLD_HEADER.trim_start_matches("// ");
        assert!(first.contains(words), "{}: {first}", path.display());
    }
    for (path, text) in rust_templates() {
        assert!(text.starts_with(SCAFFOLD_HEADER), "{}", path.display());
    }
}

#[test]
fn templates_have_no_carriage_returns_or_placeholders() {
    for (path, text) in skeleton() {
        assert!(!text.contains('\r'), "{}", path.display());
        for placeholder in [CRATE_PLACEHOLDER, RULES_PLACEHOLDER] {
            assert!(!text.contains(placeholder), "{}", path.display());
        }
    }
}

#[test]
fn rust_templates_never_unwrap_expect_or_panic() {
    for (path, text) in rust_templates() {
        // Rule 6 bans `println!`; `main.rs` reports a failure to start on
        // stderr with `eprintln!`.
        let text = text.replace("eprintln!", "");
        for banned in [
            ".unwrap()",
            ".expect(",
            "panic!",
            "todo!",
            "unimplemented!",
            "println!",
            "dbg!",
        ] {
            assert!(!text.contains(banned), "{} uses {banned}", path.display());
        }
    }
}

#[test]
fn rust_templates_use_only_the_allowed_crates() {
    let allowed = BTreeSet::from([
        "actix_web",
        "crate",
        "jsonwebtoken",
        "serde",
        "sqlx",
        "std",
        "thiserror",
        "tracing",
        // `main.rs` logs through it.
        "tracing_subscriber",
        // The OpenAPI document and Swagger UI.
        "utoipa",
        "utoipa_swagger_ui",
        "uuid",
        // The generated data layer needs it for `derive = ["validate"]`,
        // and `AppError::Validation` carries its errors.
        "validator",
        // `main.rs` starts the application through its library.
        CRATE,
    ]);
    for (path, text) in rust_templates() {
        for line in text.lines() {
            if let Some(used) = line.trim().strip_prefix("use ") {
                let root = used.split("::").next().unwrap_or_default();
                assert!(allowed.contains(root), "{}: {line}", path.display());
            }
        }
    }
}

#[test]
fn the_config_file_turns_on_the_api_for_actix_web() -> Result<()> {
    let dir = TempDir::new()?;
    let file = dir.path().join("rok-sqlc.toml");
    fs::write(&file, template("rok-sqlc.toml"))?;
    let config = config_file::load(&file)?;
    assert_eq!(config.schema, vec![dir.path().join("migrations/")]);
    assert_eq!(config.output, Some(dir.path().join("src/db")));
    assert_eq!(config.derive, Some(vec![Derive::Serde, Derive::Validate]));
    assert_eq!(config.extractors, Some(vec![Extractor::ActixWeb]));
    assert_eq!(
        config.conventions.resolve().primary_key_ids,
        Some(IdGeneration::UuidV7)
    );
    assert_eq!(
        config.api.resolve().map(|api| api.output),
        Some(dir.path().join("src/features"))
    );
    Ok(())
}

#[test]
fn the_config_file_hints_the_cargo_dependencies() {
    let text = template("rok-sqlc.toml");
    for dependency in [
        "# actix-web = \"4\"",
        "# jsonwebtoken = ",
        "# serde = ",
        "# sqlx = { version = \"0.9\"",
        "# thiserror = ",
        "# tracing = ",
        "# tracing-subscriber = { version = \"0.3\", features = [\"env-filter\"] }",
        "# uuid = ",
        "# validator = ",
    ] {
        assert!(text.contains(dependency), "missing {dependency}");
    }
}

#[test]
fn the_library_declares_the_modules_and_main_starts_the_server() {
    let lib = template("src/lib.rs");
    let main = template("src/main.rs");
    for module in ["app", "config", "db", "error", "features", "shared"] {
        assert!(
            lib.contains(&format!("\npub mod {module};\n")),
            "mod {module}"
        );
        assert!(
            !main.contains(&format!("mod {module};")),
            "main: mod {module}"
        );
    }
    let text = main;
    assert!(text.contains("\nuse my_app::{app, config};\n"));
    assert!(text.contains("#[actix_web::main]"));
    assert!(text.contains("config::Config::from_env()"));
    assert!(text.contains("app::run(config).await"));
    // It logs, filtered by `RUST_LOG` (read in `config.rs`), and reports a
    // failure to start in one readable line and a non-zero exit.
    assert!(text.contains("tracing_subscriber::fmt()"));
    assert!(text.contains("EnvFilter::new(config::log_filter())"));
    assert!(text.contains("eprintln!(\"error: {err}\")"));
    assert!(text.contains("ExitCode::FAILURE"));
    assert!(!text.contains("io::Error::other"), "no Debug dump");
    assert!(
        !text.contains("HttpServer"),
        "main.rs only starts the server"
    );
}

#[test]
fn app_wires_the_generated_features_and_health() {
    let text = template("src/app.rs");
    for wiring in [
        "use crate::db::pool::init_pool;",
        "use crate::features::{configure_all, register_services};",
        "use crate::shared::health;",
        "init_pool(&config.database_url, &config.pool)",
        "register_services(cfg, pool);",
        "health::routes(cfg);",
        "configure_all(cfg);",
        "HttpServer::new(",
    ] {
        assert!(text.contains(wiring), "missing {wiring}");
    }
}

#[test]
fn config_is_the_only_environment_reader() {
    for (path, text) in rust_templates() {
        let reads_env = text.contains("env::var") || text.contains("std::env");
        assert_eq!(
            reads_env,
            path == Path::new("src/config.rs"),
            "{}",
            path.display()
        );
    }
    let text = template("src/config.rs");
    for setting in [
        "\"DATABASE_URL\"",
        "\"BIND_ADDR\"",
        "\"JWT_SECRET\"",
        "\"RUST_LOG\"",
        "PoolSettings::default()",
        "pub fn from_env() -> Result<Self, ConfigError>",
    ] {
        assert!(text.contains(setting), "missing {setting}");
    }
}

#[test]
fn app_error_answers_every_code_of_the_wire_standard() {
    let text = template("src/error.rs");
    let codes = [
        ("BadRequest", "bad_request", "BAD_REQUEST"),
        ("Unauthorized", "unauthorized", "UNAUTHORIZED"),
        ("Forbidden", "forbidden", "FORBIDDEN"),
        ("NotFound", "not_found", "NOT_FOUND"),
        ("Conflict", "conflict", "CONFLICT"),
        ("PayloadTooLarge", "payload_too_large", "PAYLOAD_TOO_LARGE"),
        ("Validation", "validation_failed", "UNPROCESSABLE_ENTITY"),
        (
            "PreconditionRequired",
            "precondition_required",
            "PRECONDITION_REQUIRED",
        ),
        ("VersionConflict", "version_conflict", "PRECONDITION_FAILED"),
        ("Internal", "internal_error", "INTERNAL_SERVER_ERROR"),
    ];
    for (variant, code, status) in codes {
        let pattern = if variant == "Validation" {
            "AppError::Validation { .. }".to_owned()
        } else {
            format!("AppError::{variant}(_)")
        };
        assert!(
            text.contains(&format!("{pattern} => \"{code}\"")),
            "{variant} code"
        );
        assert!(
            text.contains(&format!("{pattern} => StatusCode::{status}")),
            "{variant} status"
        );
    }
    assert!(text.contains("impl ResponseError for AppError"));
    assert!(text.contains("use crate::shared::response::{ErrorBody, ErrorPayload};"));
    assert!(
        text.contains("tracing::error!"),
        "internal errors are logged"
    );
}

#[test]
fn app_error_maps_database_errors() {
    let text = template("src/error.rs");
    assert!(text.contains("impl From<sqlx::Error> for AppError"));
    for (kind, variant) in [
        ("sqlx::Error::RowNotFound", "AppError::NotFound("),
        ("ErrorKind::UniqueViolation", "AppError::Conflict("),
        ("ErrorKind::ForeignKeyViolation", "AppError::Conflict("),
        ("ErrorKind::CheckViolation", "AppError::Validation {"),
    ] {
        let after = text.split(kind).nth(1).unwrap_or_default();
        let arm = after.split(";\n").next().unwrap_or_default();
        assert!(arm.contains(variant), "{kind} becomes {variant}");
    }
    assert!(
        text.contains("database.constraint()"),
        "names the constraint"
    );
    assert!(text.contains("AppError::Internal(Box::new(error))"));
}

#[test]
fn auth_verifies_an_hs256_bearer_token() {
    let text = template("src/shared/auth.rs");
    for piece in [
        "#[serde(rename_all = \"snake_case\")]\npub enum Role {",
        "pub struct AuthUser {\n    /// The token's `sub`.\n    pub id: Uuid,",
        "pub role: Role,",
        "pub struct AdminUser(pub AuthUser);",
        "impl FromRequest for AuthUser",
        "impl FromRequest for AdminUser",
        "type Error = AppError;",
        "Algorithm::HS256",
        "header::AUTHORIZATION",
        "strip_prefix(\"Bearer \")",
        "AppError::Unauthorized(",
        "AppError::Forbidden(",
    ] {
        assert!(text.contains(piece), "missing {piece}");
    }
}

#[test]
fn shared_and_features_declare_the_generated_modules() {
    let shared = template("src/shared/mod.rs");
    for module in [
        "auth",
        "health",
        "pagination",
        "response",
        "tenant",
        "validation",
    ] {
        assert!(
            shared.contains(&format!("\npub mod {module};\n")),
            "{module}"
        );
    }
    let features = template("src/features/mod.rs");
    assert!(features.ends_with("\nmod generated;\npub use generated::*;\n"));
}

#[test]
fn agents_md_holds_the_rules_and_where_code_goes() {
    let text = template("AGENTS.md")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for section in [
        "## Where does each piece of code go?",
        "## The six rules",
        "service_api.rs",
        "service.rs",
        "port",
        "The data layer is exempt",
        "One service method does one thing; name it as a verb.",
    ] {
        assert!(text.contains(section), "missing {section}");
    }
}

/// `AGENTS.md` states exactly what `check-arch` enforces: its rules section
/// is rendered from `check-arch`'s own rule texts.
#[test]
fn agents_md_states_the_rules_check_arch_enforces() {
    let raw = template("AGENTS.md");
    assert!(
        raw.contains(&format!(
            "## The six rules\n\n{}\n\n",
            crate::check_arch::rules_markdown()
        )),
        "AGENTS.md's rules section is not check-arch's"
    );
    let text = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    for rule in [
        "1. In a feature, only `routes.rs` and `handlers.rs` use `actix_web`.",
        "6. No `unwrap()`, `expect()`, `panic!`, `todo!`, `println!` or `dbg!`.",
        "no file or directory is named `utils`, `helpers` or `common`",
        "Test code is exempt from every rule",
        "the directories under `api.output` (`src/features/` by default)",
        "the module `api.db_path` names (`crate::db`, in `src/db/`, by default)",
    ] {
        assert!(text.contains(rule), "AGENTS.md does not say: {rule}");
    }
    for file in crate::check_arch::FEATURE_FILES {
        assert!(
            raw.contains(&format!("─ {file} ")),
            "the layout lacks {file}"
        );
    }
}

#[test]
fn openapi_merges_the_generated_document_into_yours() {
    let text = template("src/openapi.rs");
    for expected in [
        "use crate::features::GeneratedApiDoc;",
        "#[derive(OpenApi)]",
        "pub struct ApiDoc;",
        "pub fn document() -> utoipa::openapi::OpenApi {",
        "doc.merge(GeneratedApiDoc::openapi());",
        "env!(\"CARGO_PKG_NAME\")",
        "env!(\"CARGO_PKG_VERSION\")",
    ] {
        assert!(text.contains(expected), "missing {expected}\n{text}");
    }
    assert!(template("src/lib.rs").contains("\npub mod openapi;\n"));
}

#[test]
fn app_serves_the_document_and_swagger_ui() {
    let text = template("src/app.rs");
    for wiring in [
        "use utoipa_swagger_ui::SwaggerUi;",
        "use crate::openapi::document;",
        "SwaggerUi::new(\"/docs/{_:.*}\").url(\"/api-docs/openapi.json\", document())",
        "web::redirect(\"/docs\", \"/docs/\")",
    ] {
        assert!(text.contains(wiring), "missing {wiring}\n{text}");
    }
    // Vendored: its build script would otherwise download Swagger UI.
    assert!(template("rok-sqlc.toml").contains(
        "# utoipa-swagger-ui = { version = \"9\", features = [\"actix-web\", \"vendored\"] }\n"
    ));
}
