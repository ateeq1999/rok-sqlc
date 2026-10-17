//! The PostgreSQL smoke test written into the fixture crate.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;

use anyhow::Result;

use crate::generate::{Outcome, dependency_order, harness_header, subdirectories, up_migrations};
use crate::{db_dir, manifest_dir};

/// The names of the `pub async fn`s in a generated Rust file, in order.
pub(super) fn generated_functions(rust: &str) -> Vec<String> {
    rust.lines()
        .filter_map(|line| line.strip_prefix("pub async fn "))
        .filter_map(|rest| rest.split(['<', '(']).next())
        .map(str::to_owned)
        .collect()
}

/// Renders `src/db/smoke.rs`: the migrations of every migration-based
/// fixture in dependency order, and a `run` that calls `list`, `count` and
/// `search_after` for every generated table that has them.
pub(super) fn render_smoke(outcomes: &[Outcome]) -> Result<String> {
    let migration_based: BTreeMap<&str, &Outcome> = outcomes
        .iter()
        .filter(|outcome| outcome.fixture.migrations)
        .map(|outcome| (outcome.fixture.module.as_str(), outcome))
        .collect();

    let mut text = harness_header();
    text.push_str(
        "//! Smoke checks for the generated reads of every migration-based module fixture.\n\n\
         // Harness code, not generated code: it names each filter as `Default::default()`.\n\
         #![allow(clippy::default_trait_access)]\n\n",
    );
    text.push_str(
        "/// Every migration-based fixture's up migrations as `(path, sql)`, in dependency order.\n",
    );
    text.push_str("pub const MIGRATIONS: &[(&str, &str)] = &[\n");
    for module in dependency_order(&migration_based.keys().copied().collect())? {
        for path in up_migrations(&migration_based[module.as_str()].fixture.schema)? {
            let label = path
                .strip_prefix(manifest_dir())
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let absolute = path.to_string_lossy().into_owned();
            writeln!(text, "    ({label:?}, include_str!({absolute:?})),")?;
        }
    }
    text.push_str("];\n\n");

    text.push_str(
        "/// Calls every generated table's `list`, `count` and `search_after` and returns\n\
         /// one line per failed call.\n",
    );
    text.push_str("pub async fn run(pool: &sqlx::PgPool) -> Vec<String> {\n");
    text.push_str("    let mut failures = Vec::new();\n");
    for (module, outcome) in &migration_based {
        let root = db_dir().join(&outcome.fixture.id);
        for table in subdirectories(&root)? {
            let read = root.join(&table).join("read.rs");
            if !read.is_file() {
                continue;
            }
            let source = fs::read_to_string(&read)?;
            let functions = generated_functions(&source);
            let path = format!("super::modules::{module}::{table}::read");
            for (function, args) in [
                ("list", ""),
                ("count", ""),
                (
                    "search_after",
                    &*format!(
                        ", Default::default(), None, super::modules::{module}::search::Limit::new(10)"
                    ),
                ),
            ] {
                if functions.iter().any(|name| name == function) {
                    // A tenant table's reads take the tenant right after the
                    // executor; the nil tenant owns no rows, which still runs
                    // every statement.
                    let tenant = match tenant_type(&source, function).as_deref() {
                        None => "",
                        Some("uuid::Uuid") => ", uuid::Uuid::nil()",
                        Some(_) => ", uuid::Uuid::nil().into()",
                    };
                    let args = format!("pool{tenant}{args}");
                    writeln!(
                        text,
                        "    record(&mut failures, \"{module}::{table}::{function}\", {path}::{function}({args}).await);"
                    )?;
                }
            }
        }
    }
    text.push_str("    failures\n}\n\n");
    text.push_str(
        "fn record<T>(failures: &mut Vec<String>, call: &str, result: Result<T, sqlx::Error>) {\n\
         \x20   if let Err(err) = result {\n\
         \x20       failures.push(format!(\"{call}: {err}\"));\n\
         \x20   }\n\
         }\n",
    );
    Ok(text)
}

/// The type of the `rok` preset's tenant parameter of the generated `function`
/// in `rust` (a raw id or a typed id), or `None` when it takes no tenant.
fn tenant_type(rust: &str, function: &str) -> Option<String> {
    let tenant = rok_sqlc::Conventions::rok()
        .tenant_column
        .expect("the rok preset has a tenant column");
    rust.split(&format!("pub async fn {function}<"))
        .nth(1)
        .and_then(|rest| rest.split(") ->").next())
        .and_then(|params| params.split(&format!("{tenant}: ")).nth(1))
        .and_then(|rest| rest.split([',', '\n']).next())
        .map(|tenant_type| tenant_type.trim().to_owned())
}
