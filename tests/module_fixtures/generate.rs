//! Discovering the fixtures and generating each into the fixture crate.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

use crate::expected::{KEPT, KeptScenario};
use crate::smoke::{generated_functions, render_smoke};
use crate::{db_dir, manifest_dir};

/// One generator input taken from the `modules/` corpus.
pub(super) struct Fixture {
    /// The corpus module, e.g. `hr`.
    pub(super) module: String,
    /// The output directory under `src/db`, e.g. `modules/hr`.
    pub(super) id: String,
    /// The `migrations/` directory or the legacy `schema.sql`.
    pub(super) schema: PathBuf,
    queries: Option<PathBuf>,
    /// Whether the fixture is migration-based (and so applied to PostgreSQL).
    pub(super) migrations: bool,
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

pub(super) fn subdirectories(dir: &Path) -> Result<Vec<String>> {
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
pub(super) struct Outcome {
    pub(super) fixture: Fixture,
    /// Every generated file, written or unchanged.
    pub(super) files: Vec<PathBuf>,
    /// Rejected tables as `(table, reason)`.
    pub(super) tables: Vec<(String, String)>,
    /// Rejected queries as `(query, reason)`.
    pub(super) queries: Vec<(String, String)>,
    /// Generated query functions, in file order.
    pub(super) functions: Vec<String>,
}

/// Regenerates the whole `src/db` tree of the fixture crate: every fixture,
/// the `mod.rs` files that tie them together, and the PostgreSQL smoke
/// module.
pub(super) fn generate_all() -> Result<Vec<Outcome>> {
    let db = db_dir();
    if db.exists() {
        fs::remove_dir_all(&db).with_context(|| format!("failed to clear {}", db.display()))?;
    }

    let mut outcomes = Vec::new();
    for fixture in discover()? {
        outcomes.push(generate_fixture(fixture)?);
    }
    for scenario in KEPT {
        outcomes.push(generate_kept(scenario)?);
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
    root.push_str("//! The generated code of every module fixture.\n\n");
    for (group, modules) in &groups {
        root.push_str(&format!("pub mod {group};\n"));
        let mut text = harness_header();
        text.push_str(&format!("//! The `{group}` fixtures.\n\n"));
        // Each fixture's `db` module is named after the fixture, so a table
        // named like its fixture (`customers::customers`) nests a module in
        // its namesake; a consuming crate's `db` module never does.
        text.push_str("#![allow(clippy::module_inception)]\n\n");
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

pub(super) fn harness_header() -> String {
    "// Written by the module fixture harness (tests/module_fixtures.rs).\n\n".to_owned()
}

fn generate_fixture(fixture: Fixture) -> Result<Outcome> {
    let mut config = rok_sqlc::Config::default();
    config.schema = vec![fixture.schema.clone()];
    config.queries = fixture.queries.iter().cloned().collect();
    config.output = db_dir().join(&fixture.id);
    config.force = true;
    config.lenient = true;
    // The migration-based fixtures are the Rok ERP, which the `rok` preset
    // describes; the legacy single-file schemas predate its conventions.
    if fixture.migrations {
        config.conventions = rok_sqlc::Conventions::rok();
    }

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

/// The up migrations of a fixture's `migrations/` directory, in the
/// generator's order: filename order, `*.down.sql` skipped.
pub(super) fn up_migrations(dir: &Path) -> Result<Vec<PathBuf>> {
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
pub(super) fn dependency_order(modules: &BTreeSet<&str>) -> Result<Vec<String>> {
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

pub(super) fn modules_dir() -> PathBuf {
    manifest_dir().join("modules")
}

/// Generates a [`KeptScenario`] into `src/db/kept/<name>` from migrations
/// written under `target/kept-fixtures/<name>`.
pub(super) fn generate_kept(scenario: &KeptScenario) -> Result<Outcome> {
    let root = manifest_dir()
        .join("target/kept-fixtures")
        .join(scenario.name);
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }
    let migrations = root.join("migrations");
    fs::create_dir_all(&migrations)?;
    fs::write(migrations.join("0001_init.up.sql"), scenario.initial)?;
    let fixture = Fixture {
        module: scenario.name.to_owned(),
        id: format!("kept/{}", scenario.name),
        schema: migrations.clone(),
        queries: None,
        migrations: false,
    };
    let mut config = rok_sqlc::Config::default();
    config.schema = vec![migrations.clone()];
    config.output = db_dir().join(&fixture.id);
    config.force = true;
    rok_sqlc::generate(&config)
        .with_context(|| format!("kept scenario `{}` failed to generate", scenario.name))?;

    fs::write(migrations.join("0002_break.up.sql"), scenario.breaking)?;
    config.lenient = true;
    let report = rok_sqlc::generate(&config)
        .with_context(|| format!("kept scenario `{}` failed to regenerate", scenario.name))?;
    ensure!(
        config.output.join("orders/models.rs").is_file(),
        "kept scenario `{}` lost the kept `orders` files",
        scenario.name
    );
    Ok(Outcome {
        files: report.written.into_iter().chain(report.unchanged).collect(),
        tables: report
            .table_failures
            .into_iter()
            .map(|failure| (failure.table, failure.reason))
            .collect(),
        queries: Vec::new(),
        functions: Vec::new(),
        fixture,
    })
}
