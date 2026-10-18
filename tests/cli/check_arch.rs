//! `rok-sqlc check-arch` checks a project against ADR 0016's architecture:
//! the feature layout, and the guidelines' rules 1, 2, 4, 5 and 6 plus the
//! cross-feature write rule, read from the Rust source with `syn`. Each
//! violation is printed as `path:line: rule: message`; any fails the run.

use super::*;

/// The config of a project `init` scaffolded: features in `src/features`.
const CONFIG: &str = "[api]\noutput = \"src/features\"\n";

/// A scratch project with `rok-sqlc.toml` and `files`.
fn project(files: &[(&str, &str)]) -> Result<TempDir> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("rok-sqlc.toml"), CONFIG)?;
    for (path, text) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap_or(dir.path()))?;
        fs::write(path, text)?;
    }
    Ok(dir)
}

/// Runs `rok-sqlc check-arch <args>` in `dir`.
fn check_arch(dir: &TempDir, args: &[&str]) -> Output {
    let mut full = vec!["check-arch"];
    full.extend(args);
    run_binary_in(dir.path(), &full)
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

/// Asserts the run failed and printed exactly `lines` as violations.
fn assert_violations(output: &Output, lines: &[&str]) {
    assert!(
        !output.status.success(),
        "expected a failure, stdout:\n{}",
        stdout(output)
    );
    let printed: Vec<String> = stdout(output).lines().map(str::to_owned).collect();
    assert_eq!(
        printed,
        lines,
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A feature that follows every rule: what `rok-sqlc g` generates, in
/// short.
const NOTES: &[(&str, &str)] = &[
    (
        "src/features/notes/mod.rs",
        "pub mod handlers;\npub mod repo;\npub mod routes;\npub mod service;\n",
    ),
    (
        "src/features/notes/routes.rs",
        "use actix_web::web;\npub fn routes(cfg: &mut web::ServiceConfig) { let _ = cfg; }\n",
    ),
    (
        "src/features/notes/handlers.rs",
        "use actix_web::HttpResponse;\npub async fn list() -> HttpResponse { HttpResponse::Ok().finish() }\n",
    ),
    (
        "src/features/notes/repo.rs",
        "pub use crate::db::notes::read::*;\npub use crate::db::notes::write::*;\n",
    ),
    (
        "src/features/notes/service.rs",
        "use sqlx::{PgConnection, PgPool};\nuse crate::db::labels::read::get;\n\
         pub struct NotesService { pub pool: PgPool }\n\
         pub fn conn(_: &mut PgConnection) {}\n",
    ),
];

#[test]
fn a_project_that_follows_the_rules_passes() -> Result<()> {
    let dir = project(NOTES)?;
    let output = check_arch(&dir, &[]);
    assert_success(&output);
    assert_eq!(stdout(&output), "");
    Ok(())
}

#[test]
fn rule_1_actix_web_outside_routes_and_handlers_is_reported() -> Result<()> {
    let dir = project(NOTES)?;
    fs::write(
        dir.path().join("src/features/notes/service.rs"),
        "// actix_web in a comment is fine.\nconst S: &str = \"actix_web::web\";\n\
         fn f() -> actix_web::HttpResponse {\n    todo_free()\n}\n",
    )?;
    assert_violations(
        &check_arch(&dir, &[]),
        &[
            "src/features/notes/service.rs:3: rule-1: `actix_web::HttpResponse`: only routes.rs, handlers.rs, app.rs, main.rs, error.rs, features/generated.rs, shared/ and db/ may use actix_web",
        ],
    );
    Ok(())
}

#[test]
fn rule_2_sqlx_outside_the_data_files_is_reported() -> Result<()> {
    let mut files = NOTES.to_vec();
    // The data layer, shared/ and the wiring may.
    files.push(("src/db/notes/read.rs", "use sqlx::PgExecutor;\n"));
    files.push(("src/shared/health.rs", "use sqlx::PgPool;\n"));
    files.push(("src/app.rs", "use sqlx::postgres::PgPoolOptions;\n"));
    let dir = project(&files)?;
    // service.rs may name the pool and the connection, nothing else.
    fs::write(
        dir.path().join("src/features/notes/service.rs"),
        "use sqlx::{PgPool, Row};\npub struct S { pub pool: PgPool }\n",
    )?;
    fs::write(
        dir.path().join("src/features/notes/handlers.rs"),
        "pub async fn list(pool: &sqlx::PgPool) { let _ = pool; }\n",
    )?;
    assert_violations(
        &check_arch(&dir, &[]),
        &[
            "src/features/notes/handlers.rs:1: rule-2: `sqlx::PgPool`: only repo.rs, model.rs, app.rs, error.rs, features/generated.rs, shared/ and db/ may use sqlx (service.rs and service_api.rs: PgPool and PgConnection only)",
            "src/features/notes/service.rs:1: rule-2: `sqlx::Row`: only repo.rs, model.rs, app.rs, error.rs, features/generated.rs, shared/ and db/ may use sqlx (service.rs and service_api.rs: PgPool and PgConnection only)",
        ],
    );
    Ok(())
}

#[test]
fn a_path_through_a_use_is_reported_once_at_the_use() -> Result<()> {
    let dir = project(NOTES)?;
    fs::write(
        dir.path().join("src/features/notes/dto.rs"),
        "use actix_web::web;\n\npub fn f(_: web::Json<()>) {}\n",
    )?;
    let output = check_arch(&dir, &[]);
    assert_violations(
        &output,
        &[
            "src/features/notes/dto.rs:1: rule-1: `actix_web::web`: only routes.rs, handlers.rs, app.rs, main.rs, error.rs, features/generated.rs, shared/ and db/ may use actix_web",
        ],
    );
    Ok(())
}

#[test]
fn rule_4_a_feature_referring_to_another_feature_is_reported() -> Result<()> {
    let mut files = NOTES.to_vec();
    files.push(("src/features/labels/mod.rs", "pub mod service;\n"));
    files.push((
        "src/features/labels/service.rs",
        "pub struct LabelsService;\n",
    ));
    let dir = project(&files)?;
    fs::write(
        dir.path().join("src/features/notes/dto.rs"),
        "use super::service::NotesService;\n\
         use crate::features::notes::repo;\n\
         use crate::features::configure_all;\n\
         use crate::features::labels::service::LabelsService;\n\
         pub fn f() { super::super::labels::service::LabelsService; }\n",
    )?;
    assert_violations(
        &check_arch(&dir, &[]),
        &[
            "src/features/notes/dto.rs:4: rule-4: `crate::features::labels::service::LabelsService`: feature `notes` refers to another feature; declare a port in its service.rs instead",
            "src/features/notes/dto.rs:5: rule-4: `crate::features::labels::service::LabelsService`: feature `notes` refers to another feature; declare a port in its service.rs instead",
        ],
    );
    Ok(())
}

#[test]
fn rule_5_shared_referring_to_the_features_is_reported() -> Result<()> {
    let mut files = NOTES.to_vec();
    files.push(("src/shared/mod.rs", "pub mod mail;\n"));
    files.push((
        "src/shared/mail.rs",
        "use crate::db::pool::PoolSettings;\nuse crate::features::notes::NotesService;\n",
    ));
    let dir = project(&files)?;
    assert_violations(
        &check_arch(&dir, &[]),
        &[
            "src/shared/mail.rs:2: rule-5: `crate::features::notes::NotesService`: shared/ refers to the features",
        ],
    );
    Ok(())
}

#[test]
fn rule_6_panics_and_prints_outside_tests_are_reported() -> Result<()> {
    let dir = project(NOTES)?;
    fs::write(
        dir.path().join("src/config.rs"),
        "// x.unwrap() in a comment, and panic! too.\n\
         pub fn f(x: Option<u8>) -> u8 {\n\
         \x20   let s = \"y.expect(\\\"no\\\")\";\n\
         \x20   println!(\"{}\", x.unwrap());\n\
         \x20   x.expect(\"a value\")\n\
         }\n\
         pub fn g() { dbg!(1); todo!() }\n\
         pub fn h() { panic!(\"no\") }\n\
         pub fn fine(x: Option<u8>) -> u8 { x.unwrap_or_default() }\n\
         #[cfg(test)]\n\
         mod tests {\n\
         \x20   #[test]\n\
         \x20   fn t() { Some(1).unwrap(); panic!(); }\n\
         }\n",
    )?;
    let message = |what: &str| format!("rule-6: `{what}` outside tests: return an error instead");
    assert_violations(
        &check_arch(&dir, &[]),
        &[
            &format!("src/config.rs:4: {}", message("println!")),
            &format!("src/config.rs:4: {}", message("unwrap()")),
            &format!("src/config.rs:5: {}", message("expect()")),
            &format!("src/config.rs:7: {}", message("dbg!")),
            &format!("src/config.rs:7: {}", message("todo!")),
            &format!("src/config.rs:8: {}", message("panic!")),
        ],
    );
    Ok(())
}

/// `unwrap` and `expect` called as paths (`Option::unwrap(x)`) count like
/// method calls; other functions, and names that are not called, do not.
#[test]
fn rule_6_unwrap_and_expect_called_through_a_path_are_reported() -> Result<()> {
    let dir = project(NOTES)?;
    fs::write(
        dir.path().join("src/config.rs"),
        "use std::option::Option::unwrap as _;\n\
         pub fn f(x: Option<u8>, r: Result<u8, ()>) -> u8 {\n\
         \x20   let a = Option::unwrap(x);\n\
         \x20   let b = Result::expect(r, \"a value\");\n\
         \x20   let c = std::option::Option::<u8>::unwrap(x);\n\
         \x20   let d = unwrap(x);\n\
         \x20   a + b + c + d\n\
         }\n\
         pub fn fine(x: Option<u8>) -> u8 {\n\
         \x20   let unwrap = Option::unwrap_or_default(x);\n\
         \x20   let expect = unwrap_or(x, 1);\n\
         \x20   let expectation = expected(unwrap);\n\
         \x20   expect + expectation\n\
         }\n",
    )?;
    let message = |what: &str| format!("rule-6: `{what}` outside tests: return an error instead");
    assert_violations(
        &check_arch(&dir, &[]),
        &[
            &format!("src/config.rs:3: {}", message("unwrap()")),
            &format!("src/config.rs:4: {}", message("expect()")),
            &format!("src/config.rs:5: {}", message("unwrap()")),
            &format!("src/config.rs:6: {}", message("unwrap()")),
        ],
    );
    Ok(())
}

#[test]
fn writing_another_tables_data_is_reported_and_reading_it_is_not() -> Result<()> {
    let dir = project(NOTES)?;
    fs::write(
        dir.path().join("src/features/notes/service.rs"),
        "use crate::db::labels::read::get;\n\
         use crate::db::notes::write::create;\n\
         use crate::db::labels;\n\
         pub fn f() { labels::write::delete(); crate::db::labels::write::create(); }\n",
    )?;
    let message = |path: &str| {
        format!(
            "`{path}`: feature `notes` writes another table; declare a port in its service.rs and implement it in app.rs"
        )
    };
    assert_violations(
        &check_arch(&dir, &[]),
        &[
            &format!(
                "src/features/notes/service.rs:4: cross-feature-write: {}",
                message("crate::db::labels::write::create")
            ),
            &format!(
                "src/features/notes/service.rs:4: cross-feature-write: {}",
                message("crate::db::labels::write::delete")
            ),
        ],
    );
    Ok(())
}

/// A project breaking each layout check once.
fn badly_laid_out() -> Result<TempDir> {
    let mut files = NOTES.to_vec();
    files.push(("src/features/notes/validators.rs", "pub fn v() {}\n"));
    files.push(("src/features/notes/extra/mod.rs", "pub fn v() {}\n"));
    files.push(("src/shared/utils.rs", "pub fn v() {}\n"));
    files.push(("src/helpers/mod.rs", "pub fn v() {}\n"));
    files.push(("src/bin/worker.rs", "fn main() {}\n"));
    files.push(("src/db/mod.rs", "pub mod pool;\n"));
    files.push(("src/features/README.md", "generated\n"));
    files.push((
        "src/features/generated.rs",
        "use actix_web::web;\nuse sqlx::PgPool;\n",
    ));
    project(&files)
}

const LAYOUT: &[&str] = &[
    "src/features/notes/extra:1: layout: feature `notes` holds a file it may not: a feature has only mod.rs, routes.rs, handlers.rs, dto.rs, model.rs, repo.rs, service_api.rs, service.rs",
    "src/features/notes/validators.rs:1: layout: feature `notes` holds a file it may not: a feature has only mod.rs, routes.rs, handlers.rs, dto.rs, model.rs, repo.rs, service_api.rs, service.rs",
    "src/helpers:1: layout: no module may be named `helpers`: put the code in shared/ or in a feature",
    "src/helpers:1: layout: unknown top-level directory `helpers`: src/ holds only bin, db, features, shared",
    "src/shared/utils.rs:1: layout: no module may be named `utils`: put the code in shared/ or in a feature",
];

#[test]
fn layout_violations_are_reported() -> Result<()> {
    let dir = badly_laid_out()?;
    assert_violations(&check_arch(&dir, &[]), LAYOUT);
    Ok(())
}

#[test]
fn lenient_turns_layout_violations_into_warnings() -> Result<()> {
    let dir = badly_laid_out()?;
    for flag in ["--lenient", "-l"] {
        let output = check_arch(&dir, &[flag]);
        assert_success(&output);
        assert_eq!(stdout(&output), "");
        let stderr = String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n");
        let warnings: Vec<String> = LAYOUT
            .iter()
            .map(|line| format!("warning: {line}"))
            .collect();
        assert_eq!(stderr.lines().collect::<Vec<_>>(), warnings);
    }

    // Rules still fail the run.
    fs::write(dir.path().join("src/config.rs"), "pub fn f() { todo!() }\n")?;
    let output = check_arch(&dir, &["-l"]);
    assert_violations(
        &output,
        &["src/config.rs:1: rule-6: `todo!` outside tests: return an error instead"],
    );
    Ok(())
}

#[test]
fn the_config_file_says_where_the_features_and_the_data_layer_are() -> Result<()> {
    let dir = project(&[
        ("src/api/mod.rs", "pub mod notes;\n"),
        ("src/api/notes/mod.rs", "pub mod service;\n"),
        (
            "src/api/notes/service.rs",
            "use crate::data::labels::write::create;\n",
        ),
        ("src/data/mod.rs", "pub mod labels;\n"),
    ])?;
    fs::write(
        dir.path().join("rok-sqlc.toml"),
        "[api]\noutput = \"src/api\"\ndb_path = \"crate::data\"\n",
    )?;
    let output = check_arch(&dir, &["-c", "rok-sqlc.toml"]);
    assert_violations(
        &output,
        &[
            "src/api/notes/service.rs:1: cross-feature-write: `crate::data::labels::write::create`: feature `notes` writes another table; declare a port in its service.rs and implement it in app.rs",
        ],
    );
    Ok(())
}

#[test]
fn a_project_without_api_output_is_an_error() -> Result<()> {
    let dir = project(NOTES)?;
    fs::write(
        dir.path().join("rok-sqlc.toml"),
        "schema = \"migrations/\"\n",
    )?;
    let output = check_arch(&dir, &["--config", "rok-sqlc.toml"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("api.output"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
