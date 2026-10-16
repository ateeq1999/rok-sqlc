//! The module fixture harness: generates every module fixture under
//! `modules/` into the fixture crate `tests/modules-app`, then checks the
//! crate the way a consumer would see it.
//!
//! - A module with `migrations/` generates into `src/db/modules/<module>/`
//!   from those migrations (filename order, `*.down.sql` skipped).
//! - A module with a single-file `schema.sql` generates into
//!   `src/db/legacy_modules/<module>/` from it and the `queries.sql` written
//!   against it, where one exists.
//!
//! Every fixture generates leniently, so a table or query the generator
//! rejects is skipped rather than fatal; [`EXPECTED`] lists each rejection,
//! and any difference from it fails the harness.
//!
//! Set `ROK_SQLC_TEST_DATABASE_URL` to a PostgreSQL server to also apply
//! every fixture's migrations, in dependency order, to a fresh database and
//! smoke-run each generated table's reads (see
//! `tests/modules-app/tests/postgres.rs`). The URL's role must be able to
//! create databases; the database it names is not touched.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use anyhow::{Context, Result, bail, ensure};

mod common;

/// Every test regenerates into the same fixture crate, so they run one at a time.
static APP: Mutex<()> = Mutex::new(());

/// What the generator is expected to reject in one fixture, and the query
/// functions it is expected to generate. Fixtures not listed here must
/// generate every table and query.
struct Expected {
    /// The fixture's directory under `src/db`, e.g. `legacy_modules/hr`.
    fixture: &'static str,
    /// Rejected tables, each with a fragment of the reason.
    tables: &'static [(&'static str, &'static str)],
    /// Rejected queries, each with a fragment of the reason, in file order.
    queries: &'static [(&'static str, &'static str)],
    /// Generated query functions, in file order.
    functions: &'static [&'static str],
}

const EXPECTED: &[Expected] = &[
    Expected {
        fixture: "legacy_modules/accounting",
        tables: &[],
        queries: &[],
        functions: &["create_account", "list_accounts", "deactivate_account"],
    },
    Expected {
        fixture: "legacy_modules/hr",
        tables: &[(
            "employees",
            // `employment_type` now maps to `types::EmploymentType`; `INTERVAL`
            // is still unsupported.
            "column `tenure`: type `INTERVAL` is not supported yet",
        )],
        queries: &[(
            "ListEmployeesByDepartment",
            "table `employees` is not generatable",
        )],
        functions: &["create_department"],
    },
    Expected {
        fixture: "legacy_modules/inventory",
        tables: &[],
        queries: &[],
        functions: &[
            "create_product",
            "create_products",
            "update_product_stock",
            "delete_product",
            "list_products_by_category",
            "list_products_offset",
            "upsert_product",
        ],
    },
    Expected {
        fixture: "legacy_modules/sales",
        tables: &[],
        queries: &[("CreateOrderWithItems", "contains 4 statements")],
        functions: &[
            "create_customer",
            "list_orders_cursor",
            "update_order_status",
        ],
    },
];

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn modules_dir() -> PathBuf {
    manifest_dir().join("modules")
}

fn app_dir() -> PathBuf {
    manifest_dir().join("tests/modules-app")
}

fn db_dir() -> PathBuf {
    app_dir().join("src/db")
}

/// One generator input taken from the `modules/` corpus.
struct Fixture {
    /// The corpus module, e.g. `hr`.
    module: String,
    /// The output directory under `src/db`, e.g. `modules/hr`.
    id: String,
    /// The `migrations/` directory or the legacy `schema.sql`.
    schema: PathBuf,
    queries: Option<PathBuf>,
    /// Whether the fixture is migration-based (and so applied to PostgreSQL).
    migrations: bool,
}

/// Every fixture in the corpus, ordered by output directory.
fn discover() -> Result<Vec<Fixture>> {
    let mut fixtures = Vec::new();
    for module in subdirectories(&modules_dir())? {
        let dir = modules_dir().join(&module);
        if dir.join("migrations").is_dir() {
            fixtures.push(Fixture {
                id: format!("modules/{module}"),
                module: module.clone(),
                schema: dir.join("migrations"),
                queries: None,
                migrations: true,
            });
        }
        if dir.join("schema.sql").is_file() {
            let queries = dir.join("queries.sql");
            fixtures.push(Fixture {
                id: format!("legacy_modules/{module}"),
                module: module.clone(),
                schema: dir.join("schema.sql"),
                queries: queries.is_file().then_some(queries),
                migrations: false,
            });
        }
    }
    fixtures.sort_by(|a, b| a.id.cmp(&b.id));
    ensure!(
        !fixtures.is_empty(),
        "no module fixtures found under modules/"
    );
    Ok(fixtures)
}

fn subdirectories(dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        if entry.path().is_dir() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    Ok(names)
}

/// What generating one fixture produced.
struct Outcome {
    fixture: Fixture,
    /// Every generated file, written or unchanged.
    files: Vec<PathBuf>,
    /// Rejected tables as `(table, reason)`.
    tables: Vec<(String, String)>,
    /// Rejected queries as `(query, reason)`.
    queries: Vec<(String, String)>,
    /// Generated query functions, in file order.
    functions: Vec<String>,
}

/// Regenerates the whole `src/db` tree of the fixture crate: every fixture,
/// the `mod.rs` files that tie them together, and the PostgreSQL smoke
/// module.
fn generate_all() -> Result<Vec<Outcome>> {
    let db = db_dir();
    if db.exists() {
        fs::remove_dir_all(&db).with_context(|| format!("failed to clear {}", db.display()))?;
    }

    let mut outcomes = Vec::new();
    for fixture in discover()? {
        outcomes.push(generate_fixture(fixture)?);
    }

    let mut groups: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for outcome in &outcomes {
        let (group, module) = outcome
            .fixture
            .id
            .split_once('/')
            .expect("fixture id has a group");
        groups.entry(group).or_default().push(module);
    }
    let mut root = harness_header();
    for (group, modules) in &groups {
        root.push_str(&format!("pub mod {group};\n"));
        let mut text = harness_header();
        for module in modules {
            text.push_str(&format!("pub mod {module};\n"));
        }
        fs::write(db.join(group).join("mod.rs"), text)?;
    }
    root.push_str("pub mod smoke;\n");
    fs::write(db.join("mod.rs"), root)?;
    fs::write(db.join("smoke.rs"), render_smoke(&outcomes)?)?;
    Ok(outcomes)
}

fn harness_header() -> String {
    "// Written by the module fixture harness (tests/module_fixtures.rs).\n\n".to_owned()
}

fn generate_fixture(fixture: Fixture) -> Result<Outcome> {
    let mut config = rok_sqlc::Config::default();
    config.schema = vec![fixture.schema.clone()];
    config.queries = fixture.queries.iter().cloned().collect();
    config.output = db_dir().join(&fixture.id);
    config.force = true;
    config.lenient = true;

    let report = rok_sqlc::generate(&config)
        .with_context(|| format!("fixture `{}` failed to generate", fixture.id))?;
    let queries_rs = config.output.join("queries.rs");
    let functions = if fixture.queries.is_some() {
        generated_functions(&fs::read_to_string(&queries_rs)?)
    } else {
        Vec::new()
    };
    Ok(Outcome {
        files: report.written.into_iter().chain(report.unchanged).collect(),
        tables: report
            .table_failures
            .into_iter()
            .map(|failure| (failure.table, failure.reason))
            .collect(),
        queries: report
            .query_failures
            .into_iter()
            .map(|failure| (failure.query, failure.reason))
            .collect(),
        functions,
        fixture,
    })
}

/// The names of the `pub async fn`s in a generated Rust file, in order.
fn generated_functions(rust: &str) -> Vec<String> {
    rust.lines()
        .filter_map(|line| line.strip_prefix("pub async fn "))
        .filter_map(|rest| rest.split(['<', '(']).next())
        .map(str::to_owned)
        .collect()
}

/// Renders `src/db/smoke.rs`: the migrations of every migration-based
/// fixture in dependency order, and a `run` that calls `list`, `count` and
/// `search_after` for every generated table that has them.
fn render_smoke(outcomes: &[Outcome]) -> Result<String> {
    let migration_based: BTreeMap<&str, &Outcome> = outcomes
        .iter()
        .filter(|outcome| outcome.fixture.migrations)
        .map(|outcome| (outcome.fixture.module.as_str(), outcome))
        .collect();

    let mut text = harness_header();
    text.push_str(
        "//! Smoke checks for the generated reads of every migration-based module fixture.\n\n",
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
            let functions = generated_functions(&fs::read_to_string(&read)?);
            let path = format!("super::modules::{module}::{table}::read");
            for (function, args) in [
                ("list", "pool"),
                ("count", "pool"),
                (
                    "search_after",
                    &*format!(
                        "pool, Default::default(), None, super::modules::{module}::search::Limit::new(10)"
                    ),
                ),
            ] {
                if functions.iter().any(|name| name == function) {
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

/// The up migrations of a fixture's `migrations/` directory, in the
/// generator's order: filename order, `*.down.sql` skipped.
fn up_migrations(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<_>>()?;
    files.retain(|path| {
        let name = path.file_name().unwrap().to_string_lossy().to_lowercase();
        name.ends_with(".sql") && !name.ends_with(".down.sql")
    });
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    Ok(files)
}

/// Orders `modules` so every module follows the modules it requires
/// (`depends_on` in its `module.toml`); ties go alphabetically. Optional
/// dependencies name modules a module works with, in either direction, so
/// they do not order installation.
fn dependency_order(modules: &BTreeSet<&str>) -> Result<Vec<String>> {
    let mut pending: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for module in modules {
        let manifest = modules_dir().join(module).join("module.toml");
        let text = fs::read_to_string(&manifest)
            .with_context(|| format!("failed to read {}", manifest.display()))?;
        let required = toml_string_list(&text, "depends_on");
        for dependency in &required {
            ensure!(
                modules.contains(dependency.as_str()),
                "module `{module}` depends on `{dependency}`, which has no migrations"
            );
        }
        pending.insert(module.to_string(), required.into_iter().collect());
    }

    let mut order = Vec::new();
    while !pending.is_empty() {
        let Some(next) = pending
            .iter()
            .find(|(_, dependencies)| dependencies.is_empty())
            .map(|(module, _)| module.clone())
        else {
            bail!("module dependency cycle among {:?}", pending.keys());
        };
        pending.remove(&next);
        for dependencies in pending.values_mut() {
            dependencies.remove(&next);
        }
        order.push(next);
    }
    Ok(order)
}

/// The strings of a one-line `key = ["a", "b"]` entry in a `module.toml`.
fn toml_string_list(text: &str, key: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix(key)?
                .trim_start()
                .strip_prefix('=')
        })
        .flat_map(|list| {
            list.trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .map(|item| item.trim().trim_matches('"').to_owned())
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn cargo(args: &[&str]) -> Command {
    let mut command = Command::new("cargo");
    command.args(args).current_dir(app_dir()).env(
        "CARGO_TARGET_DIR",
        manifest_dir().join("target/fixture-target"),
    );
    command
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    APP.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn module_fixtures_reject_exactly_the_expected_tables_and_queries() -> Result<()> {
    let _guard = lock();
    let outcomes = generate_all()?;

    for expected in EXPECTED {
        ensure!(
            outcomes
                .iter()
                .any(|outcome| outcome.fixture.id == expected.fixture),
            "EXPECTED lists `{}`, which is not a module fixture",
            expected.fixture
        );
    }

    let mut mismatches = Vec::new();
    for outcome in &outcomes {
        let id = &outcome.fixture.id;
        let expected = EXPECTED.iter().find(|expected| expected.fixture == *id);
        let tables = expected.map_or(&[][..], |expected| expected.tables);
        let queries = expected.map_or(&[][..], |expected| expected.queries);
        let functions = expected.map_or(&[][..], |expected| expected.functions);

        if !rejections_match(&outcome.tables, tables) {
            mismatches.push(format!(
                "`{id}`: expected rejected tables {tables:?}, got {:?}",
                outcome.tables
            ));
        }
        if !rejections_match(&outcome.queries, queries) {
            mismatches.push(format!(
                "`{id}`: expected rejected queries {queries:?}, got {:?}",
                outcome.queries
            ));
        }
        if outcome.functions != functions {
            mismatches.push(format!(
                "`{id}`: expected query functions {functions:?}, got {:?}",
                outcome.functions
            ));
        }
    }
    ensure!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    Ok(())
}

/// Whether `actual` rejections are exactly `expected`, in order, each
/// reason containing its expected fragment.
fn rejections_match(actual: &[(String, String)], expected: &[(&str, &str)]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|((name, reason), (expected_name, fragment))| {
                name == expected_name && reason.contains(fragment)
            })
}

#[test]
fn module_fixtures_compile_without_warnings() -> Result<()> {
    let _guard = lock();
    if let Some(reason) = common::generated_code_skip_reason(&app_dir()) {
        eprintln!("{reason}");
        return Ok(());
    }
    let outcomes = generate_all()?;
    ensure!(
        outcomes
            .iter()
            .filter(|outcome| outcome.fixture.migrations)
            .count()
            >= 25,
        "expected the 25-module migration corpus"
    );

    // The fixture crate is `#![deny(warnings)]`; `--all-targets` also
    // type-checks the PostgreSQL smoke test.
    let status = cargo(&["check", "--quiet", "--all-targets"])
        .status()
        .context("failed to run cargo check in tests/modules-app")?;
    ensure!(
        status.success(),
        "generated module fixtures failed cargo check"
    );
    Ok(())
}

#[test]
fn module_fixtures_are_rustfmt_clean() -> Result<()> {
    let _guard = lock();
    let outcomes = generate_all()?;

    // One rustfmt run per fixture keeps the command line short enough for
    // Windows.
    let mut unformatted = Vec::new();
    for outcome in &outcomes {
        ensure!(
            !outcome.files.is_empty(),
            "fixture `{}` generated no files",
            outcome.fixture.id
        );
        let output = Command::new("rustfmt")
            .args(["--edition", "2024", "--check"])
            .args(&outcome.files)
            .output()
            .context("failed to run rustfmt (install it with `rustup component add rustfmt`)")?;
        if !output.status.success() {
            unformatted.push(format!(
                "`{}`:\n{}{}",
                outcome.fixture.id,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    ensure!(
        unformatted.is_empty(),
        "generated code is not rustfmt-clean:\n{}",
        unformatted.join("\n")
    );
    Ok(())
}

#[test]
fn module_fixtures_read_from_migrated_postgres() -> Result<()> {
    let _guard = lock();
    if let Some(reason) = common::generated_code_skip_reason(&app_dir()) {
        eprintln!("{reason}");
        return Ok(());
    }
    if std::env::var_os("ROK_SQLC_TEST_DATABASE_URL").is_none() {
        eprintln!(
            "skipping: set ROK_SQLC_TEST_DATABASE_URL to smoke-run the module fixtures against PostgreSQL"
        );
        return Ok(());
    }
    generate_all()?;

    let status = cargo(&["test", "--quiet", "--test", "postgres"])
        .status()
        .context("failed to run cargo test in tests/modules-app")?;
    ensure!(
        status.success(),
        "generated module fixtures failed against the migrated corpus"
    );
    Ok(())
}
