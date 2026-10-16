use crate::schema::{Column, Table};

use super::search::SearchApi;
use super::sql::{column_clause, key_order, key_predicate, table_ref};
use super::{
    HEADER, StaticQuery, keys, param_name, push_build_and_fetch, push_doc, push_fn_head,
    types_import, use_list, use_paths,
};

pub fn render(table: &Table) -> String {
    let api = SearchApi::for_table(table);
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
        render_search(&mut output, table);
    }
    if api.search_after {
        render_search_after(&mut output, table);
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
    if !table.primary_key.is_empty() {
        value_types.push(keys::key_type(table));
    }
    if !types_import("super::super", value_types.iter().map(String::as_str)).is_empty() {
        paths.push("super::super::types".to_owned());
    }
    paths.extend(keys::id_imports(table, lookup_columns(), true));
    if !local.is_empty() {
        paths.push(format!("super::filter::{}", braced(local)));
    }
    use_paths(&paths)
}

/// A `get_by_<cols>` lookup over one unique key other than the primary key.
struct Lookup<'a> {
    fn_name: String,
    columns: Vec<&'a Column>,
}

/// The table's unique-key lookups, in declaration order. No column is the
/// soft-delete column yet, so a partial unique index never gets a lookup.
fn unique_lookups(table: &Table) -> Vec<Lookup<'_>> {
    let skip_primary_key = usize::from(!table.primary_key.is_empty());
    table
        .unique_key_columns(None)
        .into_iter()
        .skip(skip_primary_key)
        .filter_map(|key| {
            let columns: Vec<&Column> = key
                .iter()
                .map(|name| table.columns.iter().find(|column| column.sql_name == *name))
                .collect::<Option<_>>()?;
            let names: Vec<&str> = columns
                .iter()
                .map(|column| column.rust_name.trim_end_matches('_'))
                .collect();
            let fn_name = format!("get_by_{}", names.join("_and_"));
            // `get_by_id` already names the primary-key lookup.
            (skip_primary_key == 0 || fn_name != "get_by_id").then_some(Lookup { fn_name, columns })
        })
        .collect()
}

/// A lookup parameter's type: text is borrowed as `&str`; anything else
/// (typed ids included) is taken by value.
fn lookup_param_type(column: &Column) -> &str {
    if column.typed_id.is_none() && column.sqlx_type == "String" {
        "&str"
    } else {
        column.value_type()
    }
}

fn render_unique_lookup(output: &mut String, table: &Table, lookup: &Lookup<'_>) {
    let mut sql = column_clause("SELECT", table);
    sql.push(format!("FROM {}", table_ref(table)));
    sql.push(format!("WHERE {}", key_predicate(&lookup.columns, 1)));

    let described: Vec<String> = lookup
        .columns
        .iter()
        .map(|column| format!("`{}`", column.sql_name))
        .collect();
    output.push('\n');
    push_doc(
        output,
        0,
        &[&format!(
            "Fetches the row with the given {}, or `None` if there is none.",
            described.join(" and ")
        )],
    );
    let params: Vec<String> = lookup
        .columns
        .iter()
        .map(|column| format!("{}: {}", param_name(column), lookup_param_type(column)))
        .collect();
    push_fn_head(
        output,
        &lookup.fn_name,
        &params,
        &format!("Result<Option<{}>, sqlx::Error>", table.struct_name),
    );
    StaticQuery {
        call: &format!("sqlx::query_as::<_, {}>", table.struct_name),
        sql,
        binds: lookup
            .columns
            .iter()
            .map(|column| param_name(column))
            .collect(),
        execute: "fetch_optional(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

/// Starts a search body: the `QueryBuilder` over the explicit column list,
/// then the filter's `WHERE` clause.
fn push_search_preamble(output: &mut String, table: &Table) {
    let mut select = column_clause("SELECT", table);
    select.push(format!("FROM {}", table_ref(table)));
    output.push_str("    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new(\n");
    output.push_str(&super::sql::raw_block(&select, 8));
    output.push_str(",\n    );\n");
    output.push_str("    filter.push_where(&mut query);\n");
}

fn render_search_after(output: &mut String, table: &Table) {
    let key_columns = table.primary_key_columns();
    let name = &table.struct_name;
    let key_type = keys::key_type(table);
    let key_pattern = keys::key_binding(table);
    let binds = keys::key_binds(table);

    output.push('\n');
    let next_page = if key_columns.len() > 1 {
        "row's `key()` to fetch the next page."
    } else {
        "row's key to fetch the next page."
    };
    push_doc(
        output,
        0,
        &[
            "Keyset pagination in primary-key order: up to `limit` rows matching `filter`",
            "whose key comes after `after` (from the start when `None`). Pass the last",
            next_page,
        ],
    );
    push_fn_head(
        output,
        "search_after",
        &[
            format!("filter: {name}Filter"),
            format!("after: Option<{key_type}>"),
            "limit: Limit".to_owned(),
        ],
        &format!("Result<Vec<{name}>, sqlx::Error>"),
    );
    push_search_preamble(output, table);
    output.push_str(&format!("    if let Some({key_pattern}) = after {{\n"));
    match key_columns.as_slice() {
        [key] => {
            output.push_str(&format!(
                "        query.push({});\n        query.push_bind({});\n",
                super::sql::str_literal(&format!(" AND {} > ", super::sql::ident(&key.sql_name))),
                binds[0]
            ));
        }
        _ => {
            output.push_str(&format!(
                "        query.push({});\n",
                super::sql::str_literal(&format!(" AND ({}) > (", key_order(&key_columns)))
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
        super::sql::str_literal(&format!(" ORDER BY {} LIMIT ", key_order(&key_columns)))
    ));
    output.push_str("    query.push_bind(limit.get());\n");
    push_build_and_fetch(output, table, "fetch_all");
    output.push_str("}\n");
}

fn render_search(output: &mut String, table: &Table) {
    let keys = table.primary_key_columns();
    let name = &table.struct_name;

    output.push('\n');
    let tie_break = if keys.is_empty() {
        "The table has no primary key, so rows that tie on the sort key come back in no particular order."
    } else {
        "Rows that tie on the sort key are ordered by primary key, so pages are stable."
    };
    push_doc(
        output,
        0,
        &[
            "Fetches one page of rows matching `filter`, ordered by `sort`.",
            tie_break,
        ],
    );
    push_fn_head(
        output,
        "search",
        &[
            format!("filter: {name}Filter"),
            format!("sort: Sort<{name}SortKey>"),
            "page: Page".to_owned(),
        ],
        &format!("Result<Vec<{name}>, sqlx::Error>"),
    );
    push_search_preamble(output, table);
    output.push_str("    query.push(\" ORDER BY \");\n");
    output.push_str("    query.push(sort.key.column());\n");
    output.push_str("    query.push(sort.direction.sql());\n");
    if !keys.is_empty() {
        output.push_str(&format!(
            "    query.push({});\n",
            super::sql::str_literal(&format!(", {}", key_order(&keys)))
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
    let keys = table.primary_key_columns();
    let mut sql = column_clause("SELECT", table);
    sql.push(format!("FROM {}", table_ref(table)));
    sql.push(format!("WHERE {}", key_predicate(&keys, 1)));

    output.push('\n');
    push_doc(
        output,
        0,
        &["Fetches the row with the given primary key, or `None` if there is none."],
    );
    push_fn_head(
        output,
        "get_by_id",
        &[keys::key_param(table)],
        &format!("Result<Option<{}>, sqlx::Error>", table.struct_name),
    );
    StaticQuery {
        call: &format!("sqlx::query_as::<_, {}>", table.struct_name),
        sql,
        binds: keys::key_binds(table),
        execute: "fetch_optional(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

fn render_exists_by_id(output: &mut String, table: &Table) {
    let keys = table.primary_key_columns();
    let sql = vec![
        "SELECT EXISTS (".to_owned(),
        "    SELECT 1".to_owned(),
        format!("    FROM {}", table_ref(table)),
        format!("    WHERE {}", key_predicate(&keys, 1)),
        ")".to_owned(),
    ];

    output.push('\n');
    push_doc(
        output,
        0,
        &["Returns whether a row with the given primary key exists."],
    );
    push_fn_head(
        output,
        "exists_by_id",
        &[keys::key_param(table)],
        "Result<bool, sqlx::Error>",
    );
    StaticQuery {
        call: "sqlx::query_scalar::<_, bool>",
        sql,
        binds: keys::key_binds(table),
        execute: "fetch_one(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

fn render_list(output: &mut String, table: &Table) {
    let keys = table.primary_key_columns();
    let mut sql = column_clause("SELECT", table);
    sql.push(format!("FROM {}", table_ref(table)));
    let doc = if keys.is_empty() {
        "Fetches every row, in no particular order."
    } else {
        sql.push(format!("ORDER BY {}", key_order(&keys)));
        "Fetches every row, ordered by primary key."
    };

    output.push('\n');
    push_doc(output, 0, &[doc]);
    push_fn_head(
        output,
        "list",
        &[],
        &format!("Result<Vec<{}>, sqlx::Error>", table.struct_name),
    );
    StaticQuery {
        call: &format!("sqlx::query_as::<_, {}>", table.struct_name),
        sql,
        binds: Vec::new(),
        execute: "fetch_all(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

fn render_list_page(output: &mut String, table: &Table) {
    let keys = table.primary_key_columns();
    let mut sql = column_clause("SELECT", table);
    sql.push(format!("FROM {}", table_ref(table)));
    sql.push(format!("ORDER BY {}", key_order(&keys)));
    sql.push("LIMIT $1".to_owned());
    sql.push("OFFSET $2".to_owned());

    output.push('\n');
    push_doc(
        output,
        0,
        &[
            "Fetches at most `limit` rows after skipping `offset`, ordered by primary key so",
            "consecutive pages neither repeat nor skip rows. Both values must be non-negative.",
        ],
    );
    push_fn_head(
        output,
        "list_page",
        &["limit: i64".to_owned(), "offset: i64".to_owned()],
        &format!("Result<Vec<{}>, sqlx::Error>", table.struct_name),
    );
    StaticQuery {
        call: &format!("sqlx::query_as::<_, {}>", table.struct_name),
        sql,
        binds: vec!["limit".to_owned(), "offset".to_owned()],
        execute: "fetch_all(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

fn render_count(output: &mut String, table: &Table) {
    output.push('\n');
    push_doc(output, 0, &["Counts every row."]);
    push_fn_head(output, "count", &[], "Result<i64, sqlx::Error>");
    StaticQuery {
        call: "sqlx::query_scalar::<_, i64>",
        sql: vec![format!("SELECT COUNT(*) FROM {}", table_ref(table))],
        binds: Vec::new(),
        execute: "fetch_one(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{column, table};
    use super::*;

    #[test]
    fn renders_get_by_id_with_explicit_columns() {
        let rendered = render(&table(
            vec![column("id", "i64"), column("email", "String")],
            &["id"],
        ));
        assert!(rendered.contains(
            "pub async fn get_by_id<'e, E>(executor: E, id: i64) -> Result<Option<User>, sqlx::Error>\nwhere\n    E: sqlx::PgExecutor<'e>,\n{\n"
        ));
        assert!(rendered.contains(
            "    sqlx::query_as::<_, User>(\n        r#\"\n        SELECT\n            id,\n            email\n        FROM users\n        WHERE id = $1\n        \"#,\n    )\n    .bind(id)\n    .fetch_optional(executor)\n    .await\n}\n"
        ));
    }

    #[test]
    fn renders_exists_list_page_and_count() {
        let rendered = render(&table(
            vec![column("id", "i64"), column("email", "String")],
            &["id"],
        ));
        assert!(rendered.contains(
            "pub async fn exists_by_id<'e, E>(executor: E, id: i64) -> Result<bool, sqlx::Error>"
        ));
        assert!(rendered.contains("sqlx::query_scalar::<_, bool>("));
        assert!(rendered.contains("        SELECT EXISTS (\n            SELECT 1\n            FROM users\n            WHERE id = $1\n        )\n"));
        assert!(rendered.contains(
            "        FROM users\n        ORDER BY id\n        \"#,\n    )\n    .fetch_all(executor)"
        ));
        assert!(rendered.contains("        ORDER BY id\n        LIMIT $1\n        OFFSET $2\n"));
        assert!(rendered.contains("    .bind(limit)\n    .bind(offset)\n"));
        assert!(rendered.contains("        SELECT COUNT(*) FROM users\n"));
    }

    #[test]
    fn composite_key_parameters_follow_key_order() {
        let rendered = render(&table(
            vec![
                column("order_id", "uuid::Uuid"),
                column("product_id", "uuid::Uuid"),
                column("quantity", "i32"),
            ],
            &["order_id", "product_id"],
        ));
        assert!(rendered.contains(
            "pub async fn get_by_id<'e, E>(executor: E, key: UserKey) -> Result<Option<User>, sqlx::Error>"
        ));
        assert!(rendered.contains("WHERE order_id = $1 AND product_id = $2"));
        assert!(rendered.contains("    .bind(key.order_id)\n    .bind(key.product_id)\n"));
        assert!(rendered.contains("ORDER BY order_id, product_id"));
    }

    #[test]
    fn quotes_reserved_and_mixed_case_identifiers_and_keeps_schema() {
        let mut users = table(
            vec![column("select", "i64"), column("Email", "String")],
            &["select"],
        );
        users.schema = Some("Auth".to_owned());
        let rendered = render(&users);
        assert!(rendered.contains("            \"select\",\n            \"Email\"\n        FROM \"Auth\".users\n        WHERE \"select\" = $1\n"));
    }

    #[test]
    fn key_params_avoid_local_name_collisions() {
        let rendered = render(&table(vec![column("limit", "i64")], &["limit"]));
        assert!(rendered.contains("get_by_id<'e, E>(executor: E, limit_key: i64)"));
        assert!(rendered.contains("WHERE \"limit\" = $1"));
        assert!(rendered.contains(".bind(limit_key)"));
    }

    #[test]
    fn without_primary_key_only_unordered_list_and_count() {
        let rendered = render(&table(vec![column("email", "String")], &[]));
        assert!(!rendered.contains("pub async fn get_by_id"));
        assert!(!rendered.contains("pub async fn exists_by_id"));
        assert!(!rendered.contains("pub async fn list_page"));
        assert!(!rendered.contains("pub async fn search_after"));
        let list = rendered.split("pub async fn list<").nth(1).unwrap();
        let list = list.split("pub async fn").next().unwrap();
        assert!(
            !list.contains("ORDER BY"),
            "list has no stable order to offer"
        );
        assert!(
            !rendered.contains("query.push(\", ")
                && rendered.contains("query.push(sort.direction.sql());"),
            "search sorts by the caller's key with no primary-key tie-break"
        );
        assert!(rendered.contains("No primary key detected"));
        assert!(rendered.contains("pub async fn list<'e, E>(executor: E)"));
        assert!(rendered.contains("pub async fn count<'e, E>(executor: E)"));
    }

    #[test]
    fn output_is_deterministic() {
        let build = || render(&table(vec![column("id", "i64")], &["id"]));
        assert_eq!(build(), build());
    }
}
