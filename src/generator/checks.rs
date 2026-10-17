//! Checks on the planned files before anything is written: imports that
//! collide, and types the serde derives cannot hold.

use std::path::Path;

use super::{QueriesModule, queries};
use crate::schema::{Table, lacks_serde};

/// Fails when the generated file at `path` would import two items of one
/// name from different modules: row structs, filters, typed ids, or key
/// structs of two tables whose struct names agree (`category` and
/// `categories` both define `Category`). Reads the file's `use` declarations,
/// which the renderers write in one of two shapes: `use a::b::Item;` and
/// `use a::b::{One, Two};`, possibly wrapped over several lines.
pub(super) fn check_imports(output: &Path, path: &Path, text: &str) -> Result<(), String> {
    let mut imported: Vec<(String, String)> = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let Some(start) = line.strip_prefix("use ") else {
            continue;
        };
        let mut declaration = start.to_owned();
        while !declaration.ends_with(';') {
            let Some(next) = lines.next() else {
                break;
            };
            declaration.push_str(next.trim());
        }
        let declaration = declaration.trim_end_matches(';');
        let (prefix, items) = match declaration.split_once("::{") {
            Some((prefix, items)) => (prefix, items.trim_end_matches('}')),
            None => declaration.rsplit_once("::").unwrap_or(("", declaration)),
        };
        for item in items
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            imported.push((item.to_owned(), prefix.to_owned()));
        }
    }
    imported.sort();
    imported.dedup();
    let Some(pair) = imported.windows(2).find(|pair| pair[0].0 == pair[1].0) else {
        return Ok(());
    };
    let file = path.strip_prefix(output).unwrap_or(path);
    let own_module = file
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    // `super::super::category::models` is `category`; `super::models` is the
    // file's own resource.
    let module = |prefix: &str| {
        prefix
            .split("::")
            .filter(|segment| !matches!(*segment, "super" | "models" | "filter"))
            .last()
            .map_or_else(|| own_module.clone(), str::to_owned)
    };
    Err(format!(
        "`{}` would import two items named `{}`, from `{}` and `{}`; rename one of the modules under `[modules]`, or rename or opt out the relationship that needs it under `[relationships]`",
        file.display().to_string().replace('\\', "/"),
        pair[0].0,
        module(&pair[0].1),
        module(&pair[1].1)
    ))
}

/// Checks that every type the serde-deriving items hold implements serde's
/// traits: table columns (rows, inputs, patches, keys, and the filter
/// conditions over them, such as `Equality<PgInterval>`), query row columns,
/// and the fields of query parameter structs. Returns the first problem,
/// naming the column or parameter.
pub(super) fn check_serde(tables: &[Table], queries: QueriesModule<'_>) -> Result<(), String> {
    let problem = |subject: String, rust_type: &str| {
        lacks_serde(rust_type).then(|| {
            format!(
                "{subject} maps to `{rust_type}`, which implements no serde traits, so \
                 `derive = [\"serde\"]` cannot derive `Serialize`/`Deserialize` for it; \
                 drop the serde derive or the column"
            )
        })
    };
    for table in tables {
        for column in &table.columns {
            let subject = format!("column `{}` of `{}`", column.sql_name, table.display_name());
            if let Some(problem) = problem(subject, &column.sqlx_type) {
                return Err(problem);
            }
        }
    }
    for query in queries.analyzed() {
        if queries::uses_params_struct(query) {
            for param in &query.params {
                let subject = format!("parameter `{}` of query `{}`", param.name, query.name);
                if let Some(problem) = problem(subject, &param.rust_type) {
                    return Err(problem);
                }
            }
        }
        if let crate::queries::ResultShape::Row(columns) = &query.result {
            for column in columns {
                let subject = format!("column `{}` of query `{}`", column.sql_name, query.name);
                if let Some(problem) = problem(subject, &column.rust_type) {
                    return Err(problem);
                }
            }
        }
    }
    Ok(())
}
