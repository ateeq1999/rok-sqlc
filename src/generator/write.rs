//! `create`, `update`, and `delete`; `attach`, `detach`, and `sync` on a join table.
//!
//! Statements whose shape is fixed at generation time are emitted as static
//! SQL. Statements whose shape depends on which optional fields are present
//! (an insert with defaulted columns, a partial update) are built at runtime
//! with `sqlx::QueryBuilder`, whose `push_bind` allocates each placeholder as
//! it binds the value, so placeholder numbers and bind order cannot drift
//! apart. Only identifiers from the schema reach the SQL text.

use crate::schema::{Column, Table};

use super::relationships::{JoinPair, join_pair};
use super::sql::{
    column_clause, first_placeholder, ident, live_condition, scoped_key_predicate, str_literal,
    table_ref, tenant_condition,
};
use super::{
    HEADER, StaticQuery, doc_name, keys, param_name, param_type, push_and_chain,
    push_build_and_fetch, push_doc, push_fn_head, push_let_call, types_import, use_list,
};

pub fn render(table: &Table) -> String {
    let insert_columns = table.insert_columns();
    let update_columns = table.update_columns();
    let keys = table.primary_key_columns();
    let has_update = !keys.is_empty() && !update_columns.is_empty();

    let mut imports = vec![table.struct_name.clone()];
    if !insert_columns.is_empty() {
        imports.push(format!("New{}", table.struct_name));
    }
    if has_update {
        imports.push(format!("Update{}", table.struct_name));
    }
    imports.extend(super::keys::key_items(table));
    imports.sort();
    // Key parameters use the key's type: an enum key column's type, never a
    // key struct's fields.
    let mut key_types: Vec<String> = table
        .tenant()
        .map(|tenant| tenant.value_type().to_owned())
        .into_iter()
        .collect();
    if !table.key_columns().is_empty() {
        key_types.push(super::keys::key_type(table));
    }
    let join = join_pair(table);
    let pair_columns: Vec<&Column> = join.iter().flat_map(|pair| [pair.near, pair.far]).collect();
    key_types.extend(
        pair_columns
            .iter()
            .map(|column| column.value_type().to_owned()),
    );
    let types = types_import("super::super", key_types.iter().map(String::as_str));
    let key_columns = table.key_columns();
    let ids = super::use_paths(&super::keys::id_imports(
        table,
        super::keys::param_columns(table)
            .into_iter()
            .chain(pair_columns.iter().copied()),
        true,
    ));
    let outcome = if has_versioned_update(table) {
        "use super::super::UpdateOutcome;\n"
    } else {
        ""
    };
    let mut output = format!("{HEADER}\n{outcome}{types}{ids}{}", use_models(&imports));

    if insert_columns.is_empty() {
        output.push_str(
            "\n// No insertable columns detected; create is not generated for this table.\n",
        );
    } else if insert_columns
        .iter()
        .any(|column| column.is_omittable_on_insert())
    {
        render_dynamic_create(&mut output, table, &insert_columns);
    } else {
        render_static_create(&mut output, table, &insert_columns);
    }

    if keys.is_empty() {
        output.push_str(
            "\n// No primary key detected; update and delete are not generated for this table.\n",
        );
    } else {
        render_key_writes(&mut output, table, &keys, &key_columns, has_update);
    }
    if let Some(pair) = &join {
        render_join_writes(&mut output, table, pair);
    }
    output
}

/// `update` and `delete` (with `restore` and `delete_permanently` on a
/// soft-delete table), for a table with a primary key.
fn render_key_writes(
    output: &mut String,
    table: &Table,
    keys: &[&Column],
    key_columns: &[&Column],
    has_update: bool,
) {
    let update_columns = table.update_columns();
    if has_update {
        match version_column(table) {
            Some(version) => {
                render_versioned_update(output, table, version, keys, &update_columns);
            }
            None => render_update(output, table, &update_columns),
        }
    } else {
        output.push_str(
            "\n// No updatable columns detected; update is not generated for this table.\n",
        );
    }
    render_delete(output, table, key_columns);
}

/// `use super::models::...;` for sorted `names`, wrapped like rustfmt wraps a
/// use list that does not fit on one line.
fn use_models(names: &[String]) -> String {
    use_list("super::models", names)
}

fn returning_clause(table: &Table) -> Vec<String> {
    column_clause("RETURNING", table)
}

fn push_create_head(output: &mut String, table: &Table, doc: &[&str]) {
    output.push('\n');
    push_doc(output, 0, doc);
    push_fn_head(
        output,
        "create",
        &keys::scoped_params(table, &[format!("input: New{}", table.struct_name)]),
        &format!("Result<{}, sqlx::Error>", table.struct_name),
    );
}

fn render_static_create(output: &mut String, table: &Table, columns: &[&Column]) {
    let names: Vec<String> = table
        .tenant()
        .into_iter()
        .chain(columns.iter().copied())
        .map(|column| ident(&column.sql_name))
        .collect();
    let placeholders: Vec<String> = (1..=names.len()).map(|index| format!("${index}")).collect();
    let mut sql = vec![
        format!("INSERT INTO {} ({})", table_ref(table), names.join(", ")),
        format!("VALUES ({})", placeholders.join(", ")),
    ];
    sql.extend(returning_clause(table));

    push_create_head(
        output,
        table,
        &["Inserts one row and returns it as stored."],
    );
    StaticQuery {
        call: &format!("sqlx::query_as::<_, {}>", table.struct_name),
        sql,
        binds: keys::tenant_bind(table)
            .into_iter()
            .chain(
                columns
                    .iter()
                    .map(|column| format!("input.{}", column.rust_name)),
            )
            .collect(),
        execute: "fetch_one(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

fn render_dynamic_create(output: &mut String, table: &Table, columns: &[&Column]) {
    push_create_head(
        output,
        table,
        &[
            "Inserts one row and returns it as stored. Defaulted columns left as `None`",
            "are omitted from the `INSERT` so the database default applies.",
        ],
    );

    // When every insertable column is defaulted, an all-`None` input lists no
    // columns at all, which only `DEFAULT VALUES` can express. A tenant table
    // always lists its tenant column.
    if table.tenant().is_none() && columns.iter().all(|column| column.is_omittable_on_insert()) {
        let terms: Vec<String> = columns
            .iter()
            .map(|column| format!("input.{}.is_none()", column.rust_name))
            .collect();
        push_and_chain(output, 4, "if ", &terms, " {");
        let mut sql = vec![
            format!("INSERT INTO {}", table_ref(table)),
            "DEFAULT VALUES".to_owned(),
        ];
        sql.extend(returning_clause(table));
        StaticQuery {
            call: &format!("sqlx::query_as::<_, {}>", table.struct_name),
            sql,
            binds: Vec::new(),
            execute: "fetch_one(executor)",
        }
        .push(output, 8, "return ", ";");
        output.push_str("    }\n\n");
    }

    push_let_call(
        output,
        4,
        "let mut query",
        "sqlx::QueryBuilder::<sqlx::Postgres>::new",
        &str_literal(&format!("INSERT INTO {} (", table_ref(table))),
    );
    output.push_str("    let mut columns = query.separated(\", \");\n");
    if let Some(tenant) = table.tenant() {
        output.push_str(&format!(
            "    columns.push({});\n",
            str_literal(&ident(&tenant.sql_name))
        ));
    }
    for column in columns {
        let name = str_literal(&ident(&column.sql_name));
        if column.is_omittable_on_insert() {
            output.push_str(&format!("    if input.{}.is_some() {{\n", column.rust_name));
            output.push_str(&format!("        columns.push({name});\n"));
            output.push_str("    }\n");
        } else {
            output.push_str(&format!("    columns.push({name});\n"));
        }
    }
    output.push_str("    query.push(\") VALUES (\");\n");
    output.push_str("    let mut values = query.separated(\", \");\n");
    if let Some(tenant) = keys::tenant_bind(table) {
        output.push_str(&format!("    values.push_bind({tenant});\n"));
    }
    for column in columns {
        if column.is_omittable_on_insert() {
            output.push_str(&format!(
                "    if let Some(value) = input.{} {{\n",
                column.rust_name
            ));
            output.push_str("        values.push_bind(value);\n");
            output.push_str("    }\n");
        } else {
            output.push_str(&format!(
                "    values.push_bind(input.{});\n",
                column.rust_name
            ));
        }
    }
    output.push_str("    query.push(\")\");\n");
    push_returning(output, table);
    push_build_and_fetch(output, table, "fetch_one");
    output.push_str("}\n");
}

fn render_update(output: &mut String, table: &Table, columns: &[&Column]) {
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
fn version_column(table: &Table) -> Option<&Column> {
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
fn render_versioned_update(
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
    output.push_str(&super::sql::raw_block(&tail, 8));
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
    let fits = |line: &str| line.len() <= super::MAX_WIDTH;

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
    if one_line.len() <= super::MAX_WIDTH {
        output.push_str(&one_line);
        output.push('\n');
    } else if chained.len() <= super::MAX_WIDTH {
        output.push_str(&format!("    query\n{chained}\n"));
    } else {
        output.push_str(&format!("    query.push(\n        {literal},\n    );\n"));
    }
}

fn render_delete(output: &mut String, table: &Table, key_columns: &[&Column]) {
    let Some(column) = table.soft_delete_column.as_deref() else {
        render_key_statement(
            output,
            table,
            "delete",
            &["Deletes the row with the given primary key and returns whether one existed."],
            vec![
                format!("DELETE FROM {}", table_ref(table)),
                format!("WHERE {}", scoped_key_predicate(table, key_columns)),
            ],
        );
        return;
    };
    let column = ident(column);
    let soft = |value: &str, condition: &str| {
        vec![
            format!("UPDATE {}", table_ref(table)),
            format!("SET {column} = {value}"),
            format!(
                "WHERE {} AND {column} {condition}",
                scoped_key_predicate(table, key_columns)
            ),
        ]
    };
    render_key_statement(
        output,
        table,
        "delete",
        &[
            &format!("Soft-deletes the row with the given primary key by setting `{column}` to"),
            "the current time, and returns whether a row changed: `false` when there is no",
            "such row or it is already deleted.",
        ],
        soft("now()", "IS NULL"),
    );
    render_key_statement(
        output,
        table,
        "restore",
        &[
            &format!(
                "Restores the soft-deleted row with the given primary key by clearing `{column}`,"
            ),
            "and returns whether a row changed: `false` when there is no such row or it is",
            "not deleted.",
        ],
        soft("NULL", "IS NOT NULL"),
    );
    render_key_statement(
        output,
        table,
        "delete_permanently",
        &[
            "Removes the row with the given primary key from the table, deleted or not, and",
            "returns whether one existed.",
        ],
        vec![
            format!("DELETE FROM {}", table_ref(table)),
            format!("WHERE {}", scoped_key_predicate(table, key_columns)),
        ],
    );
}

/// A function taking the primary key that runs `sql` and returns whether it
/// affected a row.
fn render_key_statement(
    output: &mut String,
    table: &Table,
    name: &str,
    doc: &[&str],
    sql: Vec<String>,
) {
    output.push('\n');
    push_doc(output, 0, doc);
    push_fn_head(
        output,
        name,
        &keys::key_params(table),
        "Result<bool, sqlx::Error>",
    );
    StaticQuery {
        call: "sqlx::query",
        sql,
        binds: keys::scoped_key_binds(table),
        execute: "execute(executor)",
    }
    .push(output, 4, "let result = ", "?;");
    output.push_str("    Ok(result.rows_affected() > 0)\n");
    output.push_str("}\n");
}

/// The local naming a join table's restored rows, for `attach` and `sync`.
const RESTORED: &str = "rok_sqlc_restored";

/// `attach`, `detach`, and `sync` for a join table (ticket 23): they add,
/// remove, and replace the pairs of one row of the pair's `near` table.
/// Each runs one statement (ADR 0006), so `sync` replaces the set
/// atomically inside the caller's transaction. On a soft-delete join table,
/// `detach` soft-deletes and `attach` restores the soft-deleted row rather
/// than inserting another. `attach` and `sync` insert only the pair (and the
/// tenant), so they are generated only when every other insertable column
/// can be left out.
fn render_join_writes(output: &mut String, table: &Table, pair: &JoinPair<'_>) {
    let joins = JoinSql::new(table, pair);
    let near_param = format!("{}: {}", param_name(pair.near), param_type(pair.near));
    let far_param = format!("{}: {}", param_name(pair.far), param_type(pair.far));
    let near = doc_name(&pair.near_relationship.target);
    let far = doc_name(&pair.far_relationship.target);
    let required: Vec<&Column> = table
        .insert_columns()
        .into_iter()
        .filter(|column| {
            column.sql_name != pair.near.sql_name
                && column.sql_name != pair.far.sql_name
                && !column.has_default
                && !column.nullable
        })
        .collect();

    if let Some(column) = required.first() {
        output.push_str(&format!(
            "\n// `{}` has no default, so attach and sync, which insert only the pair, are not\n// generated for this join table.\n",
            column.sql_name.replace(['\r', '\n'], " ")
        ));
    } else {
        output.push('\n');
        let mut doc = vec![
            format!("Pairs the {near} row with the {far} row, and returns whether the pair"),
            "is new: `false` when they are already paired.".to_owned(),
        ];
        if let Some(live) = &joins.soft_delete {
            doc.push(format!(
                "A soft-deleted pairing is restored by clearing `{live}`, rather than"
            ));
            doc.push("inserted again.".to_owned());
        }
        push_doc(
            output,
            0,
            &doc.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        push_fn_head(
            output,
            "attach",
            &keys::scoped_params(table, &[near_param.clone(), far_param.clone()]),
            "Result<bool, sqlx::Error>",
        );
        joins.push_attach(output);
        output.push_str("}\n");
    }

    output.push('\n');
    let doc = if let Some(live) = &joins.soft_delete {
        vec![
            format!("Unpairs the {near} row from the {far} row by setting `{live}` to the"),
            "current time, and returns whether they were paired.".to_owned(),
        ]
    } else {
        vec![
            format!("Unpairs the {near} row from the {far} row, and returns whether they"),
            "were paired.".to_owned(),
        ]
    };
    push_doc(
        output,
        0,
        &doc.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    push_fn_head(
        output,
        "detach",
        &keys::scoped_params(table, &[near_param.clone(), far_param]),
        "Result<bool, sqlx::Error>",
    );
    joins.push_detach(output);
    output.push_str("    Ok(result.rows_affected() > 0)\n}\n");

    if required.is_empty() {
        output.push('\n');
        let mut doc = vec![
            format!(
                "Replaces the {far} rows paired with the {near} row by `{}`, in one",
                joins.set
            ),
            "statement: pairs missing from it are removed, and new ones added.".to_owned(),
        ];
        if let Some(live) = &joins.soft_delete {
            doc.push(format!(
                "Removed pairings are soft-deleted (`{live}` set to the current time),"
            ));
            doc.push("and soft-deleted ones in the set are restored.".to_owned());
        }
        push_doc(
            output,
            0,
            &doc.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        push_fn_head(
            output,
            "sync",
            &keys::scoped_params(
                table,
                &[
                    near_param,
                    format!("{}: &[{}]", joins.set, pair.far.value_type()),
                ],
            ),
            "Result<(), sqlx::Error>",
        );
        joins.push_sync(output);
        output.push_str("    Ok(())\n}\n");
    }
}

/// The SQL fragments of a join table's writes.
struct JoinSql<'a> {
    table: &'a Table,
    pair: &'a JoinPair<'a>,
    /// The tenant column, when the join table has one: `$1`.
    tenant: Option<&'a Column>,
    /// The soft-delete column (identifier), when the join table has one.
    soft_delete: Option<String>,
    /// The name of `sync`'s slice parameter, e.g. `permission_keys`.
    set: String,
}

impl<'a> JoinSql<'a> {
    fn new(table: &'a Table, pair: &'a JoinPair<'a>) -> Self {
        Self {
            table,
            pair,
            tenant: table.tenant(),
            soft_delete: table.soft_delete_column.as_deref().map(ident),
            set: format!("{}s", param_name(pair.far).trim_end_matches('_')),
        }
    }

    /// The placeholder of the `near` value; the `far` value (or set) follows.
    fn first(&self) -> usize {
        first_placeholder(self.table)
    }

    /// `[t = $1 AND ]near = $n` over `alias.` columns (none when empty).
    fn side(&self, alias: &str) -> String {
        let prefix = if alias.is_empty() {
            String::new()
        } else {
            format!("{alias}.")
        };
        tenant_condition(self.table, (!alias.is_empty()).then_some(alias))
            .into_iter()
            .chain([format!(
                "{prefix}{} = ${}",
                ident(&self.pair.near.sql_name),
                self.first()
            )])
            .collect::<Vec<_>>()
            .join(" AND ")
    }

    /// `side AND far = $m`.
    fn both(&self) -> String {
        format!(
            "{} AND {} = ${}",
            self.side(""),
            ident(&self.pair.far.sql_name),
            self.first() + 1
        )
    }

    fn far(&self) -> String {
        ident(&self.pair.far.sql_name)
    }

    /// `INSERT INTO t (tenant, near, far)`.
    fn insert_into(&self) -> String {
        let columns: Vec<String> = self
            .tenant
            .into_iter()
            .chain([self.pair.near, self.pair.far])
            .map(|column| ident(&column.sql_name))
            .collect();
        format!(
            "INSERT INTO {} ({})",
            table_ref(self.table),
            columns.join(", ")
        )
    }

    /// `$1, $2[, $3]`: the tenant and `near` values.
    fn side_values(&self) -> Vec<String> {
        (1..=self.first())
            .map(|index| format!("${index}"))
            .collect()
    }

    /// `ON CONFLICT (key)[ WHERE live] DO NOTHING`: the join pair's unique
    /// key, with its partial index's condition.
    fn on_conflict(&self) -> String {
        let key: Vec<String> = self.pair.key.iter().map(|column| ident(column)).collect();
        let condition = match (&self.soft_delete, self.pair.partial) {
            (Some(live), true) => format!(" WHERE {live} IS NULL"),
            _ => String::new(),
        };
        format!("ON CONFLICT ({}){condition} DO NOTHING", key.join(", "))
    }

    fn binds(&self, last: &str) -> Vec<String> {
        keys::tenant_bind(self.table)
            .into_iter()
            .chain([param_name(self.pair.near), last.to_owned()])
            .collect()
    }

    fn push_attach(&self, output: &mut String) {
        let mut values = self.side_values();
        values.push(format!("${}", self.first() + 1));
        let values = values.join(", ");
        let binds = self.binds(&param_name(self.pair.far));
        let Some(live) = &self.soft_delete else {
            StaticQuery {
                call: "sqlx::query",
                sql: vec![
                    self.insert_into(),
                    format!("VALUES ({values})"),
                    self.on_conflict(),
                ],
                binds,
                execute: "execute(executor)",
            }
            .push(output, 4, "let result = ", "?;");
            output.push_str("    Ok(result.rows_affected() > 0)\n");
            return;
        };
        // The restore picks one soft-deleted row, and only when no live
        // pairing exists; the insert runs only when nothing was restored.
        let table = table_ref(self.table);
        let both = self.both();
        StaticQuery {
            call: "sqlx::query_scalar::<_, bool>",
            sql: vec![
                format!("WITH {RESTORED} AS ("),
                format!("    UPDATE {table}"),
                format!("    SET {live} = NULL"),
                "    WHERE ctid = (".to_owned(),
                "        SELECT ctid".to_owned(),
                format!("        FROM {table}"),
                format!("        WHERE {both} AND {live} IS NOT NULL"),
                format!(
                    "            AND NOT EXISTS (SELECT 1 FROM {table} WHERE {both} AND {live} IS NULL)"
                ),
                format!("        ORDER BY {live} DESC"),
                "        LIMIT 1".to_owned(),
                "    )".to_owned(),
                "    RETURNING 1".to_owned(),
                "), rok_sqlc_inserted AS (".to_owned(),
                format!("    {}", self.insert_into()),
                format!("    SELECT {values}"),
                format!("    WHERE NOT EXISTS (SELECT 1 FROM {RESTORED})"),
                format!("    {}", self.on_conflict()),
                "    RETURNING 1".to_owned(),
                ")".to_owned(),
                format!(
                    "SELECT EXISTS (SELECT 1 FROM {RESTORED}) OR EXISTS (SELECT 1 FROM rok_sqlc_inserted)"
                ),
            ],
            binds,
            execute: "fetch_one(executor)",
        }
        .push(output, 4, "", "");
    }

    fn push_detach(&self, output: &mut String) {
        let table = table_ref(self.table);
        let sql = match &self.soft_delete {
            Some(live) => vec![
                format!("UPDATE {table}"),
                format!("SET {live} = now()"),
                format!("WHERE {} AND {live} IS NULL", self.both()),
            ],
            None => vec![
                format!("DELETE FROM {table}"),
                format!("WHERE {}", self.both()),
            ],
        };
        StaticQuery {
            call: "sqlx::query",
            sql,
            binds: self.binds(&param_name(self.pair.far)),
            execute: "execute(executor)",
        }
        .push(output, 4, "let result = ", "?;");
    }

    fn push_sync(&self, output: &mut String) {
        let table = table_ref(self.table);
        let far = self.far();
        let set = format!("${}", self.first() + 1);
        let side = self.side("");
        let mut values = self.side_values();
        values.push("rok_sqlc_key".to_owned());
        let insert = [
            self.insert_into(),
            format!("SELECT DISTINCT {}", values.join(", ")),
            format!("FROM UNNEST({set}) AS rok_sqlc_wanted (rok_sqlc_key)"),
        ];
        let sql = match &self.soft_delete {
            None => {
                let mut sql = vec![
                    "WITH rok_sqlc_removed AS (".to_owned(),
                    format!("    DELETE FROM {table}"),
                    format!("    WHERE {side} AND {far} <> ALL({set})"),
                    ")".to_owned(),
                ];
                sql.extend(insert);
                sql.push(self.on_conflict());
                sql
            }
            Some(live) => {
                // Pairs outside the set are soft-deleted; for each pair in it
                // with no live row, one soft-deleted row is restored; the rest
                // are inserted. The three touch different rows.
                let deleted = self.side("rok_sqlc_deleted");
                let current = self.side("rok_sqlc_live");
                let mut sql = vec![
                    "WITH rok_sqlc_removed AS (".to_owned(),
                    format!("    UPDATE {table}"),
                    format!("    SET {live} = now()"),
                    format!("    WHERE {side} AND {live} IS NULL AND {far} <> ALL({set})"),
                    format!("), {RESTORED} AS ("),
                    format!("    UPDATE {table}"),
                    format!("    SET {live} = NULL"),
                    "    WHERE ctid IN (".to_owned(),
                    format!(
                        "        SELECT DISTINCT ON (rok_sqlc_deleted.{far}) rok_sqlc_deleted.ctid"
                    ),
                    format!("        FROM {table} AS rok_sqlc_deleted"),
                    format!("        WHERE {deleted}"),
                    format!("            AND rok_sqlc_deleted.{live} IS NOT NULL"),
                    format!("            AND rok_sqlc_deleted.{far} = ANY({set})"),
                    "            AND NOT EXISTS (".to_owned(),
                    format!("                SELECT 1 FROM {table} AS rok_sqlc_live"),
                    format!("                WHERE {current}"),
                    format!("                    AND rok_sqlc_live.{far} = rok_sqlc_deleted.{far}"),
                    format!("                    AND rok_sqlc_live.{live} IS NULL"),
                    "            )".to_owned(),
                    format!(
                        "        ORDER BY rok_sqlc_deleted.{far}, rok_sqlc_deleted.{live} DESC"
                    ),
                    "    )".to_owned(),
                    format!("    RETURNING {far}"),
                    ")".to_owned(),
                ];
                sql.extend(insert);
                sql.push(format!(
                    "WHERE NOT EXISTS (SELECT 1 FROM {RESTORED} WHERE {RESTORED}.{far} = rok_sqlc_key)"
                ));
                sql.push(self.on_conflict());
                sql
            }
        };
        StaticQuery {
            call: "sqlx::query",
            sql,
            binds: self.binds(&self.set),
            execute: "execute(executor)",
        }
        .push(output, 4, "", "?;");
    }
}

fn push_returning(output: &mut String, table: &Table) {
    output.push_str("    query.push(\n");
    output.push_str(&super::sql::raw_block(&returning_clause(table), 8));
    output.push_str(",\n    );\n");
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{column, defaulted, generated, nullable, table};
    use super::*;

    #[test]
    fn static_create_when_no_column_has_a_default() {
        let rendered = render(&table(
            vec![
                column("id", "uuid::Uuid"),
                column("email", "String"),
                nullable("note", "String"),
            ],
            &["id"],
        ));
        assert!(rendered.contains("use super::models::{NewUser, UpdateUser, User};"));
        assert!(rendered.contains(
            "pub async fn create<'e, E>(executor: E, input: NewUser) -> Result<User, sqlx::Error>"
        ));
        assert!(rendered.contains(
            "        INSERT INTO users (id, email, note)\n        VALUES ($1, $2, $3)\n        RETURNING\n            id,\n            email,\n            note\n"
        ));
        assert!(rendered.contains(
            "    .bind(input.id)\n    .bind(input.email)\n    .bind(input.note)\n    .fetch_one(executor)\n"
        ));
        assert!(!rendered.contains("QueryBuilder::<sqlx::Postgres>::new(\"INSERT"));
    }

    #[test]
    fn dynamic_create_omits_defaulted_columns_from_columns_and_binds_alike() {
        let rendered = render(&table(
            vec![
                generated(column("id", "i64")),
                column("email", "String"),
                defaulted(column("created_at", "chrono::DateTime<chrono::Utc>")),
                defaulted(nullable("tag", "String")),
            ],
            &["id"],
        ));
        assert!(rendered.contains(
            "    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new(\"INSERT INTO users (\");\n"
        ));
        assert!(rendered.contains("    columns.push(\"email\");\n    if input.created_at.is_some() {\n        columns.push(\"created_at\");\n    }\n    if input.tag.is_some() {\n        columns.push(\"tag\");\n    }\n"));
        assert!(rendered.contains("    values.push_bind(input.email);\n    if let Some(value) = input.created_at {\n        values.push_bind(value);\n    }\n    if let Some(value) = input.tag {\n        values.push_bind(value);\n    }\n"));
        assert!(!rendered.contains("columns.push(\"id\")"));
        assert!(
            !rendered.contains("DEFAULT VALUES"),
            "a required column means the list is never empty"
        );
        assert!(
            rendered.contains("    query.build_query_as::<User>().fetch_one(executor).await\n")
        );
    }

    #[test]
    fn dynamic_create_falls_back_to_default_values() {
        let rendered = render(&table(
            vec![
                generated(column("id", "i64")),
                defaulted(column("hits", "i32")),
                defaulted(column("created_at", "chrono::DateTime<chrono::Utc>")),
            ],
            &["id"],
        ));
        assert!(rendered.contains("    if input.hits.is_none() && input.created_at.is_none() {\n        return sqlx::query_as::<_, User>(\n            r#\"\n            INSERT INTO users\n            DEFAULT VALUES\n            RETURNING\n"));
        assert!(rendered.contains("        .fetch_one(executor)\n        .await;\n    }\n"));
    }

    #[test]
    fn update_uses_query_builder_binds_and_guards_empty_patch() {
        let rendered = render(&table(
            vec![
                column("order_id", "uuid::Uuid"),
                column("product_id", "uuid::Uuid"),
                column("quantity", "i32"),
                nullable("note", "String"),
            ],
            &["order_id", "product_id"],
        ));
        assert!(rendered.contains(
            "pub async fn update<'e, E>(\n    executor: E,\n    key: UserKey,\n    input: UpdateUser,\n) -> Result<Option<User>, sqlx::Error>"
        ));
        assert!(rendered.contains("    if input.is_empty() {\n        return super::read::get_by_id(executor, key).await;\n    }\n"));
        assert!(rendered.contains("    if let Some(value) = input.quantity {\n        assignments.push(\"quantity = \");\n        assignments.push_bind_unseparated(value);\n    }\n"));
        assert!(rendered.contains("    query.push(\" WHERE order_id = \");\n    query.push_bind(key.order_id);\n    query.push(\" AND product_id = \");\n    query.push_bind(key.product_id);\n"));
        assert!(
            !rendered.contains("sets.len()"),
            "placeholders come from QueryBuilder, not arithmetic"
        );
        assert!(rendered.contains("fetch_optional(executor)"));
    }

    #[test]
    fn update_assignments_follow_column_order() {
        let rendered = render(&table(
            vec![
                column("id", "i64"),
                column("alpha", "String"),
                column("beta", "String"),
                column("gamma", "String"),
            ],
            &["id"],
        ));
        let positions: Vec<usize> = ["alpha", "beta", "gamma"]
            .iter()
            .map(|name| {
                rendered
                    .find(&format!("assignments.push(\"{name} = \")"))
                    .unwrap()
            })
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn quotes_reserved_identifiers_in_dynamic_sql() {
        let rendered = render(&table(
            vec![
                generated(column("select", "i64")),
                defaulted(column("from", "String")),
                column("user", "String"),
            ],
            &["select"],
        ));
        assert!(rendered.contains("columns.push(r#\"\"from\"\"#);"));
        assert!(rendered.contains("assignments.push(r#\"\"user\" = \"#);"));
        assert!(rendered.contains("query.push(r#\" WHERE \"select\" = \"#);"));
    }

    #[test]
    fn delete_reports_whether_a_row_was_removed() {
        let rendered = render(&table(
            vec![column("id", "i64"), column("email", "String")],
            &["id"],
        ));
        assert!(rendered.contains(
            "pub async fn delete<'e, E>(executor: E, id: i64) -> Result<bool, sqlx::Error>"
        ));
        assert!(rendered.contains(
            "    let result = sqlx::query(\n        r#\"\n        DELETE FROM users\n        WHERE id = $1\n        \"#,\n    )\n    .bind(id)\n    .execute(executor)\n    .await?;\n    Ok(result.rows_affected() > 0)\n"
        ));
    }

    #[test]
    fn without_primary_key_only_create() {
        let rendered = render(&table(vec![column("email", "String")], &[]));
        assert!(rendered.contains("use super::models::{NewUser, User};"));
        assert!(rendered.contains("pub async fn create"));
        assert!(!rendered.contains("pub async fn update"));
        assert!(!rendered.contains("pub async fn delete"));
        assert!(rendered.contains("No primary key detected"));
    }

    #[test]
    fn without_insertable_columns_skips_create() {
        let rendered = render(&table(vec![generated(column("id", "i64"))], &["id"]));
        assert!(rendered.contains("use super::models::User;"));
        assert!(!rendered.contains("pub async fn create"));
        assert!(rendered.contains("No insertable columns detected"));
        assert!(rendered.contains("No updatable columns detected"));
        assert!(rendered.contains("pub async fn delete"));
    }

    #[test]
    fn long_model_imports_wrap_like_rustfmt() {
        let names = |base: &str| {
            vec![
                base.to_owned(),
                format!("New{base}"),
                format!("Update{base}"),
            ]
        };
        assert_eq!(
            use_models(&names("User")[..1]),
            "use super::models::User;\n"
        );
        assert_eq!(
            use_models(&names("User")),
            "use super::models::{User, NewUser, UpdateUser};\n"
        );
        assert_eq!(
            use_models(&names("ControlledSubstanceEntry")),
            "use super::models::{\n    ControlledSubstanceEntry, NewControlledSubstanceEntry, UpdateControlledSubstanceEntry,\n};\n"
        );
        // 99 columns on one line: rustfmt already wraps it (98 is the most).
        let at_limit = [
            "ExpenseCategory",
            "ExpenseCategoryId",
            "NewExpenseCategory",
            "UpdateExpenseCategory",
        ]
        .map(str::to_owned);
        assert_eq!(
            use_models(&at_limit),
            "use super::models::{\n    ExpenseCategory, ExpenseCategoryId, NewExpenseCategory, UpdateExpenseCategory,\n};\n"
        );
        // A list too long for one indented line is packed greedily, as
        // rustfmt's mixed import layout does.
        let packed = [
            "CustomFieldDefinition",
            "CustomFieldDefinitionId",
            "NewCustomFieldDefinition",
            "UpdateCustomFieldDefinition",
        ]
        .map(str::to_owned);
        assert_eq!(
            use_models(&packed),
            "use super::models::{\n    CustomFieldDefinition, CustomFieldDefinitionId, NewCustomFieldDefinition,\n    UpdateCustomFieldDefinition,\n};\n"
        );
        let huge = "VeryLongTableNameThatKeepsGoingAndGoingAndGoing";
        assert!(use_models(&names(huge)).contains(&format!("{{\n    {huge},\n    New{huge},\n")));
    }

    #[test]
    fn output_is_deterministic() {
        let build = || {
            render(&table(
                vec![column("id", "i64"), column("email", "String")],
                &["id"],
            ))
        };
        assert_eq!(build(), build());
    }
}
