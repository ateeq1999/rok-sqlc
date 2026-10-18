//! `db/<table>/listing.rs`: what an API feature's list endpoint reads, in
//! the data layer with the rest of the SQL (ADR 0016). Generated only for a
//! table whose feature serves `list`: the `XSort` keys `sort` takes, and a
//! page of rows matching `q`, with its count.

use super::feature::Feature;
use crate::generator::sql::{
    column_clause, ident, key_order, live_condition, raw_block, str_literal, table_ref,
};
use crate::generator::{HEADER, push_doc, push_fn_head, push_let_call, use_list};

/// The `search.rs` helper the listing matches `q` with, appended to
/// `db/search.rs` when `api.output` is set.
pub const CONTAINS_ANY: &str = r#"
/// Appends ` AND (<column> ILIKE <pattern> OR ...)`: rows where any of
/// `columns` contains `text`, ignoring case. `%`, `_` and `\` in `text`
/// match themselves. The API's list endpoints match `q` with it.
pub fn push_contains_any(columns: &[&str], text: &str, query: &mut QueryBuilder<Postgres>) {
    let pattern = format!("%{}%", escape_like(text));
    query.push(" AND (");
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            query.push(" OR ");
        }
        query.push(*column);
        query.push(" ILIKE ");
        query.push_bind(pattern.clone());
    }
    query.push(")");
}
"#;

/// Renders `listing.rs` for `feature`, which serves `list`.
pub fn render(feature: &Feature<'_>) -> String {
    let table = feature.table;
    let sort = feature.sort_enum();
    let searched = !feature.search.is_empty();
    let mut output = format!(
        "{HEADER}\n//! What the API's list endpoint reads: a page of rows{}, sorted by\n//! one of the [`{sort}`] keys.\n\n",
        if searched { " matching `q`" } else { "" }
    );
    let mut search_items = vec!["Page".to_owned(), "Sort".to_owned()];
    if searched {
        search_items.push("push_contains_any".to_owned());
    }
    output.push_str(&use_list("super::super::search", &search_items));
    output.push_str(&format!("use super::models::{};\n\n", feature.row()));

    render_sort_enum(&mut output, feature);
    if searched {
        let names: Vec<String> = feature
            .search
            .iter()
            .map(|column| str_literal(&ident(&column.sql_name)))
            .collect();
        output.push_str("\n/// The columns `q` matches.\n");
        let one_line = format!("const SEARCH_COLUMNS: &[&str] = &[{}];", names.join(", "));
        if one_line.len() <= crate::generator::MAX_WIDTH {
            output.push_str(&one_line);
            output.push('\n');
        } else {
            output.push_str("const SEARCH_COLUMNS: &[&str] = &[\n");
            for name in &names {
                output.push_str(&format!("    {name},\n"));
            }
            output.push_str("];\n");
        }
    }

    // `WHERE` and the live-row condition, so `q` appends ` AND (...)`.
    let filter = format!(
        "WHERE {}",
        live_condition(table, None).unwrap_or_else(|| "TRUE".to_owned())
    );
    let q_param = searched.then(|| "q: Option<&str>".to_owned());

    output.push('\n');
    let matching = if searched { " matching `q`" } else { "" };
    let keys = table.primary_key_columns();
    let mut doc = vec![format!(
        "Fetches one page of rows{matching}, ordered by `sort`{}.",
        if keys.is_empty() {
            ""
        } else {
            ", ties by primary key"
        }
    )];
    if searched {
        doc.push(
            "`q` matches a row when one of the [`SEARCH_COLUMNS`] contains it, ignoring".to_owned(),
        );
        doc.push("case; `%`, `_` and `\\` in it match themselves.".to_owned());
    }
    let doc: Vec<&str> = doc.iter().map(String::as_str).collect();
    push_doc(&mut output, 0, &doc);
    let mut params: Vec<String> = q_param.iter().cloned().collect();
    params.push(format!("sort: Option<Sort<{sort}>>"));
    params.push("page: Page".to_owned());
    push_fn_head(
        &mut output,
        "list_matching",
        &params,
        &format!("Result<Vec<{}>, sqlx::Error>", feature.row()),
    );
    let mut sql = column_clause("SELECT", table);
    sql.push(format!("FROM {}", table_ref(table)));
    sql.push(filter.clone());
    output.push_str("    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new(\n");
    output.push_str(&raw_block(&sql, 8));
    output.push_str(",\n    );\n");
    push_q(&mut output, searched);
    if keys.is_empty() {
        output.push_str(
            "    if let Some(sort) = sort {\n        query.push(\" ORDER BY \");\n        query.push(sort.key.column());\n        query.push(sort.direction.sql());\n    }\n",
        );
    } else {
        output.push_str(
            "    query.push(\" ORDER BY \");\n    if let Some(sort) = sort {\n        query.push(sort.key.column());\n        query.push(sort.direction.sql());\n        query.push(\", \");\n    }\n",
        );
        output.push_str(&format!(
            "    query.push({});\n",
            str_literal(&key_order(&keys))
        ));
    }
    output.push_str(
        "    query.push(\" LIMIT \");\n    query.push_bind(page.limit());\n    query.push(\" OFFSET \");\n    query.push_bind(page.offset());\n",
    );
    crate::generator::push_build_and_fetch(&mut output, table, "fetch_all");
    output.push_str("}\n\n");

    let doc = format!("Counts the rows{matching}: every page's rows together.");
    push_doc(&mut output, 0, &[&doc]);
    push_fn_head(
        &mut output,
        "count_matching",
        &q_param.into_iter().collect::<Vec<_>>(),
        "Result<i64, sqlx::Error>",
    );
    let count = format!("SELECT COUNT(*) FROM {} {filter}", table_ref(table));
    push_let_call(
        &mut output,
        4,
        "let mut query",
        "sqlx::QueryBuilder::<sqlx::Postgres>::new",
        &str_literal(&count),
    );
    push_q(&mut output, searched);
    output.push_str("    query.build_query_scalar::<i64>().fetch_one(executor).await\n}\n");
    output
}

/// `if let Some(q) = q { push_contains_any(...) }`, with search columns.
fn push_q(output: &mut String, searched: bool) {
    if searched {
        output.push_str(
            "    if let Some(q) = q {\n        push_contains_any(SEARCH_COLUMNS, q, &mut query);\n    }\n",
        );
    }
}

/// The `XSort` enum: the columns `sort` takes, its default, and the column
/// each key sorts by.
fn render_sort_enum(output: &mut String, feature: &Feature<'_>) {
    let sort = feature.sort_enum();
    push_doc(
        output,
        0,
        &[&format!(
            "The columns `GET {}?sort=` takes, in `snake_case`.",
            feature.url
        )],
    );
    output.push_str(
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]\n#[serde(rename_all = \"snake_case\")]\n",
    );
    output.push_str(&format!("pub enum {sort} {{\n"));
    for column in &feature.sort {
        push_doc(
            output,
            4,
            &[&format!("Sorts by the `{}` column.", column.sql_name)],
        );
        output.push_str(&format!("    {},\n", column.sort_variant()));
    }
    output.push_str("}\n\n");
    output.push_str(&format!("impl {sort} {{\n"));
    if let Some(default) = feature.default_sort() {
        push_doc(
            output,
            4,
            &["The key a list sorts by when its query names none."],
        );
        output.push_str(&format!(
            "    pub const DEFAULT: Self = Self::{};\n\n",
            default.sort_variant()
        ));
    }
    push_doc(output, 4, &["The SQL column this key sorts by."]);
    output.push_str(
        "    #[must_use]\n    pub fn column(self) -> &'static str {\n        match self {\n",
    );
    for column in &feature.sort {
        output.push_str(&format!(
            "            Self::{} => {},\n",
            column.sort_variant(),
            str_literal(&ident(&column.sql_name))
        ));
    }
    output.push_str("        }\n    }\n}\n");
}
