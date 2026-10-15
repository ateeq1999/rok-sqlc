//! The generation pipeline behind [`generate`](crate::generate): read the
//! inputs, parse and analyze them, plan the files, and write them.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use heck::ToSnakeCase;

use crate::config::Config;
use crate::output::is_generated_file;
use crate::queries::AnalyzedQuery;
use crate::report::{Error, QueryFailure, RefusedFile, Report, Result, TableFailure};
use crate::schema::Table;
use crate::{generator, output, parser, queries};

pub(crate) fn generate(config: &Config) -> Result<Report> {
    let mut report = Report::default();
    match run(config, &mut report) {
        Ok(()) => Ok(report),
        Err(err) => Err(Error::new(err, report)),
    }
}

fn run(config: &Config, report: &mut Report) -> anyhow::Result<()> {
    let output_dir = &config.output;
    let resource = config.resource.as_deref();
    let (dry_run, force, lenient) = (config.dry_run, config.force, config.lenient);

    let (sql, schema_label) = read_schema(config)?;
    let parsed = parser::parse_schema(&sql)
        .with_context(|| format!("failed to parse SQL schema `{schema_label}`"))?;

    let matches_resource =
        |table: &str| resource.is_none_or(|name| table == name || table.to_snake_case() == name);

    report.table_failures = parsed
        .failures
        .iter()
        .filter(|failure| matches_resource(&failure.table))
        .map(|failure| TableFailure {
            table: failure.table.clone(),
            reason: failure.reason.clone(),
        })
        .collect();

    if !report.table_failures.is_empty() && !lenient {
        bail!(
            "{} unsupported table(s) found; pass --lenient to skip them",
            report.table_failures.len()
        );
    }

    let mut include: HashSet<String> = parsed
        .schema
        .tables
        .iter()
        .filter(|table| {
            resource.is_none_or(|name| table.sql_name == name || table.module_name == name)
        })
        .map(|table| table.module_name.clone())
        .collect();

    if let Ok(entries) = std::fs::read_dir(output_dir) {
        for entry in entries.flatten() {
            let module = entry.file_name().to_string_lossy().into_owned();
            let already_generated = entry.path().join("models.rs").is_file();
            if already_generated
                && parsed
                    .schema
                    .tables
                    .iter()
                    .any(|table| table.module_name == module)
            {
                include.insert(module);
            }
        }
    }

    let selected: Vec<_> = parsed
        .schema
        .tables
        .iter()
        .filter(|table| include.contains(&table.module_name))
        .cloned()
        .collect();

    if selected.is_empty() && report.table_failures.is_empty() {
        bail!("no matching resource found for {resource:?}");
    }
    if selected.is_empty() {
        if resource.is_some() {
            bail!("no generatable table matches {resource:?}");
        }
        bail!("all tables are unsupported; see warnings above");
    }

    let query_inputs = query_inputs(&config.queries)?;
    if !query_inputs.is_empty() && selected.iter().any(|table| table.module_name == "queries") {
        bail!(
            "table module `queries` collides with the generated `queries` module; rename the table or omit --queries"
        );
    }
    let mut analyzed = Vec::new();
    let mut failed_files = Vec::new();
    for input in &query_inputs {
        let (queries, failures) = analyze_query_file(&input.path, &parsed, &selected)?;
        if !failures.is_empty() {
            failed_files.push(format!("`{}`", input.path.display()));
        }
        report.query_failures.extend(failures);
        analyzed.push(generator::QueryModule {
            name: input.module.clone().unwrap_or_default(),
            queries,
        });
    }
    if !report.query_failures.is_empty() && !lenient {
        let count = report.query_failures.len();
        bail!(
            "{count} unsupported quer{} in {}; pass --lenient to skip them",
            if count == 1 { "y" } else { "ies" },
            failed_files.join(", ")
        );
    }
    let queries_module = match analyzed.as_slice() {
        [single] if query_inputs[0].module.is_none() => {
            generator::QueriesModule::Generate(&single.queries)
        }
        [] if is_generated_file(&output_dir.join("queries.rs"))
            || is_generated_file(&output_dir.join("queries").join("mod.rs")) =>
        {
            generator::QueriesModule::Keep
        }
        [] => generator::QueriesModule::None,
        modules => generator::QueriesModule::GenerateModules(modules),
    };

    let files = generator::plan_files_with_queries(&sql, &selected, queries_module, output_dir);
    let applied = output::apply_files(&files, dry_run, force)?;

    report.unchanged = applied.unchanged;
    report.refused = applied
        .refused
        .into_iter()
        .map(|(path, reason)| RefusedFile { path, reason })
        .collect();
    if !report.refused.is_empty() && !dry_run {
        bail!(
            "nothing was written; {} file(s) refused (use --force to overwrite)",
            report.refused.len()
        );
    }
    report.written = applied.written;
    Ok(())
}

/// Reads the schema inputs and combines them in order. Returns the combined
/// SQL and a label naming the inputs for error messages.
fn read_schema(config: &Config) -> anyhow::Result<(String, String)> {
    if config.schema.is_empty() {
        bail!("no schema input given");
    }
    let mut sql = String::new();
    for path in &config.schema {
        if !sql.is_empty() && !sql.ends_with('\n') {
            sql.push('\n');
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read schema `{}`", path.display()))?;
        sql.push_str(&text);
    }
    let label = config
        .schema
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("`, `");
    Ok((sql, label))
}

/// One query file to analyze.
struct QueryInput {
    path: PathBuf,
    /// The generated module under `queries/`, named after the file stem;
    /// `None` when a single query file becomes `queries.rs`.
    module: Option<String>,
}

/// Expands the query inputs. A lone file keeps the single-module layout
/// (`queries.rs`); a directory (its `*.sql` files, sorted by name) or several
/// inputs produce one module per file under `queries/`.
fn query_inputs(inputs: &[PathBuf]) -> anyhow::Result<Vec<QueryInput>> {
    if let [path] = inputs
        && !path.is_dir()
    {
        return Ok(vec![QueryInput {
            path: path.clone(),
            module: None,
        }]);
    }

    let mut files = Vec::new();
    for input in inputs {
        if !input.is_dir() {
            files.push(input.clone());
            continue;
        }
        let read_error = || format!("failed to read query directory `{}`", input.display());
        let mut found = std::fs::read_dir(input)
            .with_context(read_error)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()
            .with_context(read_error)?;
        found.retain(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "sql"));
        if found.is_empty() {
            bail!(
                "query directory `{}` contains no `.sql` files",
                input.display()
            );
        }
        found.sort();
        files.extend(found);
    }

    let mut seen: HashMap<String, PathBuf> = HashMap::new();
    let mut modules = Vec::with_capacity(files.len());
    for path in files {
        let module = query_module_name(&path)?;
        if let Some(first) = seen.insert(module.clone(), path.clone()) {
            bail!(
                "query files `{}` and `{}` would both generate module `{module}`; rename one of them",
                first.display(),
                path.display()
            );
        }
        modules.push(QueryInput {
            path,
            module: Some(module),
        });
    }
    Ok(modules)
}

/// The module a query file generates under `queries/`: its stem, which must
/// already be a snake_case Rust identifier that is not a keyword.
fn query_module_name(path: &Path) -> anyhow::Result<String> {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    match parser::naming::rust_identifier(&stem) {
        Ok(ident) if ident == stem => Ok(stem),
        suggestion => {
            let hint = match suggestion {
                Ok(ident) => format!("; rename it to `{ident}.sql`"),
                Err(_) => String::new(),
            };
            bail!(
                "query file `{}`: `{stem}` is not a valid Rust module name (use lowercase letters, digits and underscores, not starting with a digit or naming a keyword){hint}",
                path.display()
            )
        }
    }
}

/// Parses and analyzes one query file. Unsupported queries come back as
/// failures for the caller to report.
fn analyze_query_file(
    path: &Path,
    parsed: &parser::ParsedSchema,
    selected: &[Table],
) -> anyhow::Result<(Vec<AnalyzedQuery>, Vec<QueryFailure>)> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read query file `{}`", path.display()))?;

    let (defs, mut failures) = queries::parse_query_file(&text);
    if defs.is_empty() && failures.is_empty() {
        bail!(
            "query file `{}` contains no `-- name:` annotated queries",
            path.display()
        );
    }
    let generated_modules = selected
        .iter()
        .map(|table| table.module_name.clone())
        .collect();
    let (analyzed, analysis_failures) = queries::analyze_queries(
        &defs,
        &parsed.schema.tables,
        &generated_modules,
        &parsed.failures,
    );
    failures.extend(analysis_failures);

    let failures = failures
        .into_iter()
        .map(|failure| QueryFailure {
            query: failure.query,
            reason: failure.reason,
        })
        .collect();
    Ok((analyzed, failures))
}
