//! The generation pipeline behind [`generate`](crate::generate): read the
//! inputs, parse and analyze them, plan the files, and write them.

use std::collections::HashSet;
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

    let analyzed = match config.queries.as_slice() {
        [] => None,
        [path] => Some(analyze_query_file(
            path, &parsed, &selected, lenient, report,
        )?),
        _ => bail!("only one query file is supported"),
    };
    let queries_module = match &analyzed {
        Some(analyzed) => generator::QueriesModule::Generate(analyzed),
        None if is_generated_file(&output_dir.join("queries.rs")) => generator::QueriesModule::Keep,
        None => generator::QueriesModule::None,
    };

    let files = generator::plan_files_with_queries(&sql, &selected, queries_module, output_dir);

    // Every file this run produces or deliberately keeps; any other
    // generated file under the output directory is stale.
    let mut keep: HashSet<PathBuf> = files.iter().map(|(path, _)| path.clone()).collect();
    if matches!(queries_module, generator::QueriesModule::Keep) {
        keep.insert(output_dir.join("queries.rs"));
    }
    let stale = output::find_stale(output_dir, &keep)?;

    if config.check {
        (report.missing, report.changed) = output::compare_files(&files)?;
        report.stale = stale;
        if report.has_drift() {
            let count = report.missing.len() + report.changed.len() + report.stale.len();
            bail!(
                "generated code is out of date: {count} file(s) drifted; run `rok-sqlc generate` to update them"
            );
        }
        report.unchanged = files.into_iter().map(|(path, _)| path).collect();
        return Ok(());
    }

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
    if !dry_run {
        output::remove_stale(output_dir, &stale)?;
    }
    report.stale = stale;
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

/// Parses and analyzes the query file. Unsupported queries are recorded in
/// the report; they abort the run unless `lenient`.
fn analyze_query_file(
    path: &Path,
    parsed: &parser::ParsedSchema,
    selected: &[Table],
    lenient: bool,
    report: &mut Report,
) -> anyhow::Result<Vec<AnalyzedQuery>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read query file `{}`", path.display()))?;
    if selected.iter().any(|table| table.module_name == "queries") {
        bail!(
            "table module `queries` collides with the generated `queries.rs`; rename the table or omit --queries"
        );
    }

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

    report.query_failures = failures
        .into_iter()
        .map(|failure| QueryFailure {
            query: failure.query,
            reason: failure.reason,
            path: failure.line.map(|_| path.to_path_buf()),
            line: failure.line,
        })
        .collect();
    if !report.query_failures.is_empty() && !lenient {
        let count = report.query_failures.len();
        bail!(
            "{count} unsupported quer{} in `{}`; pass --lenient to skip them",
            if count == 1 { "y" } else { "ies" },
            path.display()
        );
    }
    Ok(analyzed)
}
