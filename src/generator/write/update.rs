//! `update`: the partial update, and the versioned update that checks a
//! version column.

use crate::schema::{Column, Table};

use super::{push_returning, returning_clause};
use crate::generator::sql::{self, ident, live_condition, str_literal, table_ref};
use crate::generator::{
    MAX_WIDTH, keys, push_build_and_fetch, push_doc, push_fn_head, push_let_call,
};

pub(super) fn render_update(output: &mut String, table: &Table, columns: &[&Column]) {
    let mut params = keys::key_params(table);
    params.push(format!("input: Update{}", table.struct_name));

    output.push('\n');
    let mut doc = vec![
        "Applies a partial update and returns the updated row, or `None` if no row has",
        "the given primary key. An empty patch issues no `UPDATE` and returns the",
        "current row.",
    ];
    if table.soft_delete_column.is_some() {
        doc.push("A soft-deleted row is left unchanged and counts as missing.");
    }
    push_doc(output, 0, &doc);
    push_fn_head(
        output,
        "update",
        &params,
        &format!("Result<Option<{}>, sqlx::Error>", table.struct_name),
    );

    output.push_str("    if input.is_empty() {\n");
    push_get_by_id(output, table, "return ", ";", None);
    output.push_str("    }\n\n");

    push_let_call(
        output,
        4,
        "let mut query",
        "sqlx::QueryBuilder::<sqlx::Postgres>::new",
        &str_literal(&format!("UPDATE {} SET ", table_ref(table))),
    );
    push_assignments(output, columns);
    push_key_condition(output, table);
    push_returning(output, table);
    push_build_and_fetch(output, table, "fetch_optional");
    output.push_str("}\n");
}

/// Whether `table` gets a versioned `update`, which returns the shared
/// `UpdateOutcome`.
pub fn has_versioned_update(table: &Table) -> bool {
    !table.primary_key.is_empty()
        && !table.update_columns().is_empty()
        && version_column(table).is_some()
}

/// The parameter a versioned `update` takes the caller's last-seen version as.
const VERSION_PARAM: &str = "expected_version";

/// The table's version column, when it has one and the column can be
/// compared (it is not part of the key).
pub(super) fn version_column(table: &Table) -> Option<&Column> {
    let name = table.version_column.as_deref()?;
    table
        .columns
        .iter()
        .find(|column| column.sql_name == name && !column.is_primary_key)
}

/// The SQL name of the `index`-th key column as `rok_sqlc_target` exposes it,
/// prefixed so it cannot be mistaken for one of the table's own columns.
fn target_key_alias(index: usize) -> String {
    format!("rok_sqlc_key_{index}")
}

/// `update` for a table with a version column (ADR 0006): the version check
/// is part of the `UPDATE`'s condition, and the same statement reports
/// whether the row was updated, is at another version, or does not exist, so
/// the generic executor is used once.
pub(super) fn render_versioned_update(
    output: &mut String,
    table: &Table,
    version: &Column,
    key_columns: &[&Column],
    columns: &[&Column],
) {
    let mut params = keys::key_params(table);
    params.extend([
        format!("{VERSION_PARAM}: {}", version.field_type()),
        format!("input: Update{}", table.struct_name),
    ]);

    output.push('\n');
    let first = format!(
        "Applies a partial update if the row's `{}` is still `{VERSION_PARAM}`.",
        version.rust_name
    );
    let mut doc = vec![
        first.as_str(),
        "",
        "Returns the updated row, `NotFound` if no row has the given primary key, or",
        "`VersionConflict` if the row is at another version, in which case nothing",
        "changes. An empty patch issues no `UPDATE`: it returns the current row,",
        "unchanged, when the row is at the expected version.",
    ];
    if table.soft_delete_column.is_some() {
        doc.push("A soft-deleted row is left unchanged and counts as missing.");
    }
    push_doc(output, 0, &doc);
    push_fn_head(
        output,
        "update",
        &params,
        &format!("Result<UpdateOutcome<{}>, sqlx::Error>", table.struct_name),
    );

    output.push_str("    if input.is_empty() {\n");
    push_get_by_id(output, table, "let Some(row) = ", "?", Some(" else {"));
    output.push_str("            return Ok(UpdateOutcome::NotFound);\n");
    output.push_str("        };\n");
    output.push_str(&format!(
        "        if row.{} != {VERSION_PARAM} {{\n",
        version.rust_name
    ));
    output.push_str("            return Ok(UpdateOutcome::VersionConflict);\n");
    output.push_str("        }\n");
    output.push_str("        return Ok(UpdateOutcome::Updated(row));\n");
    output.push_str("    }\n\n");

    // `rok_sqlc_target` finds the row by key alone, in the statement's
    // snapshot, so a row at another version is told apart from a missing one.
    let aliases: Vec<String> = key_columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            format!("{} AS {}", ident(&column.sql_name), target_key_alias(index))
        })
        .collect();
    push_let_call(
        output,
        4,
        "let mut query",
        "sqlx::QueryBuilder::<sqlx::Postgres>::new",
        &str_literal("WITH rok_sqlc_target AS ("),
    );
    push_query_push(
        output,
        &format!("SELECT {} FROM {}", aliases.join(", "), table_ref(table)),
    );
    push_key_condition(output, table);
    push_query_push(
        output,
        &format!("), rok_sqlc_updated AS (UPDATE {} SET ", table_ref(table)),
    );
    push_assignments(output, columns);
    let joins: Vec<String> = key_columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            format!(
                "{} = rok_sqlc_target.{}",
                ident(&column.sql_name),
                target_key_alias(index)
            )
        })
        .collect();
    push_query_push(
        output,
        &format!(
            " FROM rok_sqlc_target WHERE {} AND {} = ",
            joins.join(" AND "),
            ident(&version.sql_name)
        ),
    );
    output.push_str(&format!("    query.push_bind({VERSION_PARAM});\n"));
    let mut tail = returning_clause(table);
    tail.push(")".to_owned());
    tail.push(
        "SELECT rok_sqlc_updated.*, EXISTS (SELECT 1 FROM rok_sqlc_updated) AS rok_sqlc_applied"
            .to_owned(),
    );
    tail.push("FROM rok_sqlc_target LEFT JOIN rok_sqlc_updated ON TRUE".to_owned());
    output.push_str("    query.push(\n");
    output.push_str(&sql::raw_block(&tail, 8));
    output.push_str(",\n    );\n");

    output.push_str("    let Some(row) = query.build().fetch_optional(executor).await? else {\n");
    output.push_str("        return Ok(UpdateOutcome::NotFound);\n");
    output.push_str("    };\n");
    output.push_str("    let applied: bool = sqlx::Row::try_get(&row, \"rok_sqlc_applied\")?;\n");
    output.push_str("    if !applied {\n");
    output.push_str("        return Ok(UpdateOutcome::VersionConflict);\n");
    output.push_str("    }\n");
    output.push_str("    Ok(UpdateOutcome::Updated(sqlx::FromRow::from_row(&row)?))\n");
    output.push_str("}\n");
}

/// `{lead}super::read::get_by_id(executor, <key args>).await{trail}` at an
/// indent of 8, with `else_open` (` else {`) after it for a `let`-`else`,
/// laid out as rustfmt lays it out: on one line when it fits; for a `let`,
/// with `else {` on its own line, then with the call moved below the `=`;
/// otherwise with one argument per line.
fn push_get_by_id(
    output: &mut String,
    table: &Table,
    lead: &str,
    trail: &str,
    else_open: Option<&str>,
) {
    let mut args = vec!["executor".to_owned()];
    args.extend(keys::key_args(table));
    let call = format!("super::read::get_by_id({}).await{trail}", args.join(", "));
    let else_open = else_open.unwrap_or("");
    let fits = |line: &str| line.len() <= MAX_WIDTH;

    let one_line = format!("        {lead}{call}{else_open}");
    if fits(&one_line) {
        output.push_str(&format!("{one_line}\n"));
        return;
    }
    let else_line = if else_open.is_empty() {
        String::new()
    } else {
        format!("        {}\n", else_open.trim_start())
    };
    if !else_open.is_empty() {
        let head = format!("        {lead}{call}");
        if fits(&head) {
            output.push_str(&format!("{head}\n{else_line}"));
            return;
        }
        let below = format!("            {call}");
        if fits(&below) {
            output.push_str(&format!(
                "        {}\n{below}\n{else_line}",
                lead.trim_end()
            ));
            return;
        }
    }
    output.push_str(&format!("        {lead}super::read::get_by_id(\n"));
    for arg in &args {
        output.push_str(&format!("            {arg},\n"));
    }
    output.push_str(&format!("        )\n        .await{trail}\n{else_line}"));
}

/// One `if let Some(value)` assignment per patch field, in column order.
fn push_assignments(output: &mut String, columns: &[&Column]) {
    output.push_str("    let mut assignments = query.separated(\", \");\n");
    for column in columns {
        output.push_str(&format!(
            "    if let Some(value) = input.{} {{\n",
            column.rust_name
        ));
        output.push_str(&format!(
            "        assignments.push({});\n",
            str_literal(&format!("{} = ", ident(&column.sql_name)))
        ));
        output.push_str("        assignments.push_bind_unseparated(value);\n");
        output.push_str("    }\n");
    }
}

/// ` WHERE t = $n AND k1 = $m ...`, binding the tenant (on a tenant table)
/// and the key, then ` AND <col> IS NULL` on a table with a soft-delete
/// column, so a deleted row counts as missing.
fn push_key_condition(output: &mut String, table: &Table) {
    let columns = table.tenant().into_iter().chain(table.key_columns());
    let binds = keys::scoped_key_binds(table);
    for (index, (column, bind)) in columns.zip(binds).enumerate() {
        let connective = if index == 0 { " WHERE" } else { " AND" };
        push_query_push(
            output,
            &format!("{connective} {} = ", ident(&column.sql_name)),
        );
        output.push_str(&format!("    query.push_bind({bind});\n"));
    }
    if let Some(live) = live_condition(table, None) {
        push_query_push(output, &format!(" AND {live}"));
    }
}

/// `query.push(text);`, laid out as rustfmt lays it out: on one line when it
/// fits, otherwise with `.push(...)` moved below `query`, otherwise with the
/// literal on its own line.
fn push_query_push(output: &mut String, text: &str) {
    let literal = str_literal(text);
    let one_line = format!("    query.push({literal});");
    let chained = format!("        .push({literal});");
    if one_line.len() <= MAX_WIDTH {
        output.push_str(&one_line);
        output.push('\n');
    } else if chained.len() <= MAX_WIDTH {
        output.push_str(&format!("    query\n{chained}\n"));
    } else {
        output.push_str(&format!("    query.push(\n        {literal},\n    );\n"));
    }
}
