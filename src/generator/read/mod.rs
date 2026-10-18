use crate::schema::{Column, Table};

use super::search::SearchApi;
use super::sql::{
    column_clause, first_placeholder, ident, key_order, live_key_predicate, scope_condition,
    str_literal, table_ref,
};
use super::{
    HEADER, StaticQuery, keys, param_name, param_type, push_build_and_fetch, push_doc,
    push_fn_head, types_import, use_list, use_paths,
};

pub fn render(table: &Table, tables: &[Table]) -> String {
    let api = SearchApi::for_table(table);
    let fallible = !super::search::filter_relationships(table, tables).is_empty();
    let mut output = format!("{HEADER}\n{}", search_imports(table, api));
    let mut models = vec![table.struct_name.clone()];
    models.extend(keys::key_items(table));
    models.sort();
    output.push_str(&use_list("super::models", &models));

    if table.primary_key.is_empty() {
        output.push_str(
            "\n// No primary key detected; get_by_id, exists_by_id, and list_page are not generated\n// for this table, and list returns rows in no particular order.\n",
        );
    } else {
        render_get_by_id(&mut output, table);
        render_exists_by_id(&mut output, table);
    }
    for lookup in unique_lookups(table) {
        render_unique_lookup(&mut output, table, &lookup);
    }
    render_list(&mut output, table);
    if !table.primary_key.is_empty() {
        render_list_page(&mut output, table);
    }
    render_count(&mut output, table);
    if api.search {
        render_search(&mut output, table, fallible);
    }
    if api.search_after {
        render_search_after(&mut output, table, fallible);
    }

    output
}

/// The `use` lines for the search and enum types `read.rs` needs, sorted the
/// way rustfmt sorts them (`super::super::` before `super::filter`).
fn search_imports(table: &Table, api: SearchApi) -> String {
    let name = &table.struct_name;
    let mut shared = Vec::new();
    let mut local = Vec::new();
    if api.search_after {
        shared.push("Limit");
        local.push(format!("{name}Filter"));
    }
    if api.search {
        shared.extend(["Page", "Sort"]);
        if local.is_empty() {
            local.push(format!("{name}Filter"));
        }
        local.push(format!("{name}SortKey"));
    }
    let braced = |items: Vec<String>| match items.as_slice() {
        [single] => single.clone(),
        _ => format!("{{{}}}", items.join(", ")),
    };
    let mut paths = Vec::new();
    if !shared.is_empty() {
        let shared = shared.into_iter().map(str::to_owned).collect();
        paths.push(format!("super::super::search::{}", braced(shared)));
    }
    // Key parameters and the keyset cursor use the key's type: an enum key
    // column's type, never a key struct's fields. Unique-key lookups take
    // each key column's value type.
    let lookups = unique_lookups(table);
    let lookup_columns = || {
        lookups
            .iter()
            .flat_map(|lookup| lookup.columns.iter().copied())
    };
    let mut value_types: Vec<String> = lookup_columns()
        .map(|column| column.value_type().to_owned())
        .collect();
    if !table.key_columns().is_empty() {
        value_types.push(keys::key_type(table));
    }
    value_types.extend(table.tenant().map(|tenant| tenant.value_type().to_owned()));
    if !types_import("super::super", value_types.iter().map(String::as_str)).is_empty() {
        paths.push("super::super::types".to_owned());
    }
    paths.extend(keys::id_imports(
        table,
        lookup_columns().chain(keys::param_columns(table)),
        true,
    ));
    if !local.is_empty() {
        paths.push(format!("super::filter::{}", braced(local)));
    }
    use_paths(&paths)
}

/// A `get_by_<cols>` lookup over one unique key other than the primary key.
pub(crate) struct Lookup<'a> {
    pub(crate) fn_name: String,
    pub(crate) columns: Vec<&'a Column>,
}

/// The table's unique-key lookups, in declaration order. A partial unique
/// index counts only when scoped to live rows by the soft-delete column. On
/// a tenant table the tenant parameter supplies the tenant column, so it is
/// left out of each key and of the lookup's name; a key left empty, or named
/// like an earlier lookup, gets none.
pub(crate) fn unique_lookups(table: &Table) -> Vec<Lookup<'_>> {
    let skip_primary_key = usize::from(!table.primary_key.is_empty());
    let mut names: Vec<String> = Vec::new();
    table
        .unique_key_columns(table.soft_delete_column.as_deref())
        .into_iter()
        .skip(skip_primary_key)
        .filter_map(|key| {
            let columns: Vec<&Column> = key
                .iter()
                .map(|name| table.columns.iter().find(|column| column.sql_name == *name))
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .filter(|column| !table.is_tenant_column(column))
                .collect();
            if columns.is_empty() {
                return None;
            }
            let column_names: Vec<&str> = columns
                .iter()
                .map(|column| column.rust_name.trim_end_matches('_'))
                .collect();
            let fn_name = format!("get_by_{}", column_names.join("_and_"));
            // `get_by_id` already names the primary-key lookup.
            if (skip_primary_key == 1 && fn_name == "get_by_id") || names.contains(&fn_name) {
                return None;
            }
            names.push(fn_name.clone());
            Some(Lookup { fn_name, columns })
        })
        .collect()
}

/// A read's doc comment, noting on a soft-delete table that deleted rows
/// are left out.
fn push_read_doc(output: &mut String, table: &Table, lines: &[&str]) {
    push_doc_with_note(output, table, lines, "Soft-deleted rows are left out.");
}

/// A search's doc comment, noting on a soft-delete table how to include
/// deleted rows.
fn push_search_doc(output: &mut String, table: &Table, lines: &[&str]) {
    push_doc_with_note(
        output,
        table,
        lines,
        "Soft-deleted rows are left out unless `filter.include_deleted` is set.",
    );
}

fn push_doc_with_note(output: &mut String, table: &Table, lines: &[&str], note: &str) {
    let mut lines = lines.to_vec();
    if table.soft_delete_column.is_some() {
        lines.push(note);
    }
    push_doc(output, 0, &lines);
}

fn render_unique_lookup(output: &mut String, table: &Table, lookup: &Lookup<'_>) {
    let mut sql = column_clause("SELECT", table);
    sql.push(format!("FROM {}", table_ref(table)));
    sql.push(format!(
        "WHERE {}",
        live_key_predicate(table, &lookup.columns)
    ));

    let described: Vec<String> = lookup
        .columns
        .iter()
        .map(|column| super::doc_name(&column.sql_name))
        .collect();
    output.push('\n');
    push_read_doc(
        output,
        table,
        &[&format!(
            "Fetches the row with the given {}, or `None` if there is none.",
            described.join(" and ")
        )],
    );
    let params: Vec<String> = lookup
        .columns
        .iter()
        .map(|column| format!("{}: {}", param_name(column), param_type(column)))
        .collect();
    push_fn_head(
        output,
        &lookup.fn_name,
        &keys::scoped_params(table, &params),
        &format!("Result<Option<{}>, sqlx::Error>", table.struct_name),
    );
    StaticQuery {
        call: &format!("sqlx::query_as::<_, {}>", table.struct_name),
        sql,
        binds: keys::tenant_bind(table)
            .into_iter()
            .chain(lookup.columns.iter().map(|column| param_name(column)))
            .collect(),
        execute: "fetch_optional(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

/// Starts a search body: the `QueryBuilder` over the explicit column list,
/// then the filter's `WHERE` clause and, on a tenant table, the tenant
/// condition.
fn push_search_preamble(output: &mut String, table: &Table, fallible: bool) {
    let mut select = column_clause("SELECT", table);
    select.push(format!("FROM {}", table_ref(table)));
    output.push_str("    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new(\n");
    output.push_str(&super::sql::raw_block(&select, 8));
    output.push_str(",\n    );\n");
    output.push_str(if fallible {
        "    filter.push_where(&mut query)?;\n"
    } else {
        "    filter.push_where(&mut query);\n"
    });
    if let Some(tenant) = table.tenant() {
        output.push_str(&format!(
            "    query.push({});\n    query.push_bind({});\n",
            str_literal(&format!(" AND {} = ", ident(&tenant.sql_name))),
            param_name(tenant)
        ));
    }
}

fn render_search_after(output: &mut String, table: &Table, fallible: bool) {
    let key_columns = table.key_columns();
    let name = &table.struct_name;
    let key_type = keys::key_param_type(table);
    let key_pattern = keys::key_binding(table);
    let binds = keys::key_binds(table);

    output.push('\n');
    let next_page = if key_columns.len() > 1 {
        "row's `key()` to fetch the next page."
    } else {
        "row's key to fetch the next page."
    };
    push_search_doc(
        output,
        table,
        &[
            "Keyset pagination in primary-key order: up to `limit` rows matching `filter`",
            "whose key comes after `after` (from the start when `None`). Pass the last",
            next_page,
        ],
    );
    push_fn_head(
        output,
        "search_after",
        &keys::scoped_params(
            table,
            &[
                format!("filter: {name}Filter"),
                format!("after: Option<{key_type}>"),
                "limit: Limit".to_owned(),
            ],
        ),
        &format!("Result<Vec<{name}>, sqlx::Error>"),
    );
    push_search_preamble(output, table, fallible);
    output.push_str(&format!("    if let Some({key_pattern}) = after {{\n"));
    match key_columns.as_slice() {
        [key] => {
            output.push_str(&format!(
                "        query.push({});\n        query.push_bind({});\n",
                str_literal(&format!(" AND {} > ", ident(&key.sql_name))),
                binds[0]
            ));
        }
        _ => {
            output.push_str(&format!(
                "        query.push({});\n",
                str_literal(&format!(" AND ({}) > (", key_order(&key_columns)))
            ));
            for (index, bind) in binds.iter().enumerate() {
                if index > 0 {
                    output.push_str("        query.push(\", \");\n");
                }
                output.push_str(&format!("        query.push_bind({bind});\n"));
            }
            output.push_str("        query.push(\")\");\n");
        }
    }
    output.push_str("    }\n");
    output.push_str(&format!(
        "    query.push({});\n",
        str_literal(&format!(" ORDER BY {} LIMIT ", key_order(&key_columns)))
    ));
    output.push_str("    query.push_bind(limit.get());\n");
    push_build_and_fetch(output, table, "fetch_all");
    output.push_str("}\n");
}

fn render_search(output: &mut String, table: &Table, fallible: bool) {
    let keys = table.primary_key_columns();
    let name = &table.struct_name;

    output.push('\n');
    let tie_break = if keys.is_empty() {
        "The table has no primary key, so rows that tie on the sort key come back in no particular order."
    } else {
        "Rows that tie on the sort key are ordered by primary key, so pages are stable."
    };
    push_search_doc(
        output,
        table,
        &[
            "Fetches one page of rows matching `filter`, ordered by `sort`.",
            tie_break,
        ],
    );
    push_fn_head(
        output,
        "search",
        &keys::scoped_params(
            table,
            &[
                format!("filter: {name}Filter"),
                format!("sort: Sort<{name}SortKey>"),
                "page: Page".to_owned(),
            ],
        ),
        &format!("Result<Vec<{name}>, sqlx::Error>"),
    );
    push_search_preamble(output, table, fallible);
    output.push_str("    query.push(\" ORDER BY \");\n");
    output.push_str("    query.push(sort.key.column());\n");
    output.push_str("    query.push(sort.direction.sql());\n");
    if !keys.is_empty() {
        output.push_str(&format!(
            "    query.push({});\n",
            str_literal(&format!(", {}", key_order(&keys)))
        ));
    }
    output.push_str("    query.push(\" LIMIT \");\n");
    output.push_str("    query.push_bind(page.limit());\n");
    output.push_str("    query.push(\" OFFSET \");\n");
    output.push_str("    query.push_bind(page.offset());\n");
    push_build_and_fetch(output, table, "fetch_all");
    output.push_str("}\n");
}

fn render_get_by_id(output: &mut String, table: &Table) {
    let keys = table.key_columns();
    let mut sql = column_clause("SELECT", table);
    sql.push(format!("FROM {}", table_ref(table)));
    sql.push(format!("WHERE {}", live_key_predicate(table, &keys)));

    output.push('\n');
    push_read_doc(
        output,
        table,
        &["Fetches the row with the given primary key, or `None` if there is none."],
    );
    push_fn_head(
        output,
        "get_by_id",
        &keys::key_params(table),
        &format!("Result<Option<{}>, sqlx::Error>", table.struct_name),
    );
    StaticQuery {
        call: &format!("sqlx::query_as::<_, {}>", table.struct_name),
        sql,
        binds: keys::scoped_key_binds(table),
        execute: "fetch_optional(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

fn render_exists_by_id(output: &mut String, table: &Table) {
    let keys = table.key_columns();
    let sql = vec![
        "SELECT EXISTS (".to_owned(),
        "    SELECT 1".to_owned(),
        format!("    FROM {}", table_ref(table)),
        format!("    WHERE {}", live_key_predicate(table, &keys)),
        ")".to_owned(),
    ];

    output.push('\n');
    push_read_doc(
        output,
        table,
        &["Returns whether a row with the given primary key exists."],
    );
    push_fn_head(
        output,
        "exists_by_id",
        &keys::key_params(table),
        "Result<bool, sqlx::Error>",
    );
    StaticQuery {
        call: "sqlx::query_scalar::<_, bool>",
        sql,
        binds: keys::scoped_key_binds(table),
        execute: "fetch_one(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

fn render_list(output: &mut String, table: &Table) {
    let keys = table.primary_key_columns();
    let mut sql = column_clause("SELECT", table);
    sql.push(format!("FROM {}", table_ref(table)));
    sql.extend(scope_condition(table).map(|scope| format!("WHERE {scope}")));
    let doc = if keys.is_empty() {
        "Fetches every row, in no particular order."
    } else {
        sql.push(format!("ORDER BY {}", key_order(&keys)));
        "Fetches every row, ordered by primary key."
    };

    output.push('\n');
    push_read_doc(output, table, &[doc]);
    push_fn_head(
        output,
        "list",
        &keys::tenant_params(table),
        &format!("Result<Vec<{}>, sqlx::Error>", table.struct_name),
    );
    StaticQuery {
        call: &format!("sqlx::query_as::<_, {}>", table.struct_name),
        sql,
        binds: keys::tenant_bind(table).into_iter().collect(),
        execute: "fetch_all(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

fn render_list_page(output: &mut String, table: &Table) {
    let keys = table.primary_key_columns();
    let mut sql = column_clause("SELECT", table);
    sql.push(format!("FROM {}", table_ref(table)));
    sql.extend(scope_condition(table).map(|scope| format!("WHERE {scope}")));
    sql.push(format!("ORDER BY {}", key_order(&keys)));
    let first = first_placeholder(table);
    sql.push(format!("LIMIT ${first}"));
    sql.push(format!("OFFSET ${}", first + 1));

    output.push('\n');
    push_read_doc(
        output,
        table,
        &[
            "Fetches at most `limit` rows after skipping `offset`, ordered by primary key so",
            "consecutive pages neither repeat nor skip rows. Both values must be non-negative.",
        ],
    );
    push_fn_head(
        output,
        "list_page",
        &keys::scoped_params(table, &["limit: i64".to_owned(), "offset: i64".to_owned()]),
        &format!("Result<Vec<{}>, sqlx::Error>", table.struct_name),
    );
    StaticQuery {
        call: &format!("sqlx::query_as::<_, {}>", table.struct_name),
        sql,
        binds: keys::tenant_bind(table)
            .into_iter()
            .chain(["limit".to_owned(), "offset".to_owned()])
            .collect(),
        execute: "fetch_all(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

fn render_count(output: &mut String, table: &Table) {
    output.push('\n');
    push_read_doc(output, table, &["Counts every row."]);
    push_fn_head(
        output,
        "count",
        &keys::tenant_params(table),
        "Result<i64, sqlx::Error>",
    );
    StaticQuery {
        call: "sqlx::query_scalar::<_, i64>",
        sql: vec![match scope_condition(table) {
            Some(scope) => format!("SELECT COUNT(*) FROM {} WHERE {scope}", table_ref(table)),
            None => format!("SELECT COUNT(*) FROM {}", table_ref(table)),
        }],
        binds: keys::tenant_bind(table).into_iter().collect(),
        execute: "fetch_one(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

#[cfg(test)]
mod tests;
