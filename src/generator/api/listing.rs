//! `db/<table>/listing.rs`: the reads and writes an API feature needs
//! beyond the data layer's own, kept with the rest of the SQL (ADR 0016).
//! Generated for a table whose feature serves `list` (the `XSort` keys
//! `sort` takes, and a page of rows matching `q`, with its count) or
//! `search`, has an owner (every read scoped to the caller: the list's,
//! `get_owned` and `lock_owned`), or writes a many-to-many relationship
//! (its join table's writes, which a feature may only call through its own
//! table's module).

use super::feature::{Endpoint, Feature};
use super::layout::push_signature;
use super::relations::Relation;
use crate::generator::openapi::push_to_schema;
use crate::generator::sql::{
    column_clause, first_placeholder, ident, key_order, live_condition, live_key_predicate,
    raw_block, scoped_key_predicate, str_literal, table_ref,
};
use crate::generator::{
    HEADER, StaticQuery, doc_name, keys, param_name, param_type, push_doc, push_fn_head,
    push_let_call, types_import, use_paths,
};

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

/// Renders `listing.rs` for `feature` (see [`super::has_listing`]).
pub fn render(feature: &Feature<'_>) -> String {
    let table = feature.table;
    let lists = feature.serves(Endpoint::List);
    let searches = feature.serves(Endpoint::Search);
    let searched = !feature.search.is_empty();
    let owned = feature.reads_owned_rows();
    let pairs: Vec<&Relation<'_>> = feature
        .relations
        .iter()
        .filter(|relation| relation.writes().is_some())
        .collect();
    let mut docs = Vec::new();
    if lists {
        docs.push(format!(
            "//! What the API's list endpoint reads: a page of rows{}{}, sorted by\n//! one of the [`{}`] keys.\n",
            if searched { " matching `q`" } else { "" },
            if feature.owner.is_some() {
                " the caller owns"
            } else {
                ""
            },
            feature.sort_enum()
        ));
    }
    if searches {
        docs.push(
            "//! What `POST /search` reads: a page of the rows a filter matches, and\n//! their count.\n"
                .to_owned(),
        );
    }
    if owned {
        docs.push(
            "//! The reads of one row the caller owns: another caller's row is none.\n".to_owned(),
        );
    }
    if !pairs.is_empty() {
        docs.push(
            "//! The many-to-many relationships' writes, through their join tables.\n".to_owned(),
        );
    }
    let mut output = format!("{HEADER}\n{}\n", docs.join("//!\n"));

    let mut paths = Vec::new();
    if lists || searches {
        let mut search_items = vec!["Page", "Sort"];
        if lists && searched {
            search_items.push("push_contains_any");
        }
        paths.push(format!(
            "super::super::search::{{{}}}",
            search_items.join(", ")
        ));
    }
    if searches {
        paths.push(format!("super::filter::{}", feature.db_filter()));
    }
    let mut models = vec![feature.row().to_owned()];
    if owned {
        models.extend(keys::key_items(table));
        paths.extend(keys::id_imports(table, keys::param_columns(table), true));
        if !types_import("super::super", [keys::key_type(table).as_str()]).is_empty() {
            paths.push("super::super::types".to_owned());
        }
    }
    for writes in pairs.iter().filter_map(|relation| relation.writes()) {
        paths.extend(keys::id_imports(table, writes.join.tenant(), true));
        for column in [writes.own, writes.other] {
            if let Some(id) = &column.typed_id {
                if id.module == table.module_name {
                    models.push(id.name.clone());
                } else {
                    paths.push(format!("super::super::{}::models::{}", id.module, id.name));
                }
            }
        }
    }
    models.sort();
    models.dedup();
    let models = if let [model] = models.as_slice() {
        model.clone()
    } else {
        format!("{{{}}}", models.join(", "))
    };
    paths.push(format!("super::models::{models}"));
    output.push_str(&use_paths(&paths));

    if lists || searches {
        output.push('\n');
        render_sort_enum(&mut output, feature);
    }
    if lists {
        render_list(&mut output, feature);
    }
    if searches {
        super::search::render_listing(&mut output, feature);
    }
    if owned {
        output.push('\n');
        render_owned(&mut output, feature);
    }
    for relation in pairs {
        super::relation_service::render_pair_writes(&mut output, relation);
    }
    output
}

/// The read of one row the caller owns, `get_owned`, `lock_owned`, the
/// check a write runs first, and `owned_by`, `owned` and `retain_owned`,
/// the checks of rows read some other way.
fn render_owned(output: &mut String, feature: &Feature<'_>) {
    let table = feature.table;
    let Some(owner) = feature.owner else {
        return;
    };
    let keys = table.key_columns();
    let owner_condition = format!(
        "{} = ${}",
        ident(&owner.sql_name),
        first_placeholder(table) + keys.len()
    );
    let predicate = format!("{} AND {owner_condition}", live_key_predicate(table, &keys));
    let mut params = keys::key_params(table);
    params.push("owner: uuid::Uuid".to_owned());
    let mut binds = keys::scoped_key_binds(table);
    binds.push("owner".to_owned());

    let mut sql = column_clause("SELECT", table);
    sql.push(format!("FROM {}", table_ref(table)));
    sql.push(format!("WHERE {predicate}"));
    push_doc(
        output,
        0,
        &[
            &format!(
                "Fetches the row with the given primary key if `owner` owns it (its {}),",
                doc_name(&owner.sql_name)
            ),
            "or `None`.",
        ],
    );
    push_fn_head(
        output,
        "get_owned",
        &params,
        &format!("Result<Option<{}>, sqlx::Error>", feature.row()),
    );
    StaticQuery {
        call: &format!("sqlx::query_as::<_, {}>", feature.row()),
        sql,
        binds: binds.clone(),
        execute: "fetch_optional(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n\n");

    // Soft-deleted rows too: restoring one checks its owner first.
    let sql = vec![
        "SELECT 1".to_owned(),
        format!("FROM {}", table_ref(table)),
        format!(
            "WHERE {} AND {owner_condition}",
            scoped_key_predicate(table, &keys)
        ),
        "FOR UPDATE".to_owned(),
    ];
    let mut doc = vec![
        "Returns whether `owner` owns the row with the given primary key, locking",
        "it until the transaction ends, so a write that checks first changes only",
        "a row the caller owns.",
    ];
    if table.soft_delete_column.is_some() {
        doc.push("A soft-deleted row counts.");
    }
    push_doc(output, 0, &doc);
    push_fn_head(output, "lock_owned", &params, "Result<bool, sqlx::Error>");
    StaticQuery {
        call: "sqlx::query_scalar::<_, i32>",
        sql,
        binds,
        execute: "fetch_optional(executor)",
    }
    .push(output, 4, "let row = ", "?;");
    output.push_str("    Ok(row.is_some())\n}\n");

    let row = feature.row();
    let owner_value = if owner.typed_id.is_some() {
        format!("uuid::Uuid::from(row.{})", owner.rust_name)
    } else {
        format!("row.{}", owner.rust_name)
    };
    let owner_param = "owner: uuid::Uuid".to_owned();
    output.push_str(&format!(
        "\n/// Whether `owner` owns `row` (its {}).\n#[must_use]\n",
        doc_name(&owner.sql_name)
    ));
    let params = [format!("row: &{row}"), owner_param.clone()];
    push_signature(output, 0, "pub fn owned_by", &params, "bool");
    output.push_str(&format!("    {owner_value} == owner\n}}\n"));

    output.push_str("\n/// `row`, if `owner` owns it.\n#[must_use]\n");
    let params = [format!("row: Option<{row}>"), owner_param.clone()];
    let returns = format!("Option<{row}>");
    push_signature(output, 0, "pub fn owned", &params, &returns);
    output.push_str("    row.filter(|row| owned_by(row, owner))\n}\n");

    output.push_str("\n/// Keeps the rows `owner` owns.\n");
    let params = [format!("rows: &mut Vec<{row}>"), owner_param];
    push_signature(output, 0, "pub fn retain_owned", &params, "");
    output.push_str("    rows.retain(|row| owned_by(row, owner));\n}\n");
}

/// `XSort`, `SEARCH_COLUMNS`, `list_matching` and `count_matching`.
fn render_list(output: &mut String, feature: &Feature<'_>) {
    let table = feature.table;
    let sort = feature.sort_enum();
    let searched = !feature.search.is_empty();
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
    let mut owner_param: Vec<String> = table
        .tenant()
        .map(|tenant| format!("{}: {}", param_name(tenant), param_type(tenant)))
        .into_iter()
        .collect();
    owner_param.extend(feature.owner.map(|_| "owner: uuid::Uuid".to_owned()));

    output.push('\n');
    let matching = format!(
        "{}{}",
        if feature.owner.is_some() {
            " `owner` owns"
        } else {
            ""
        },
        if searched { " matching `q`" } else { "" }
    );
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
    push_doc(output, 0, &doc);
    let mut params: Vec<String> = owner_param.iter().chain(&q_param).cloned().collect();
    params.push(format!("sort: Option<Sort<{sort}>>"));
    params.push("page: Page".to_owned());
    push_fn_head(
        output,
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
    super::search::push_scope(output, feature);
    push_q(output, searched);
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
    crate::generator::push_build_and_fetch(output, table, "fetch_all");
    output.push_str("}\n\n");

    let doc = format!("Counts the rows{matching}: every page's rows together.");
    push_doc(output, 0, &[&doc]);
    push_fn_head(
        output,
        "count_matching",
        &owner_param.into_iter().chain(q_param).collect::<Vec<_>>(),
        "Result<i64, sqlx::Error>",
    );
    let count = format!("SELECT COUNT(*) FROM {} {filter}", table_ref(table));
    push_let_call(
        output,
        4,
        "let mut query",
        "sqlx::QueryBuilder::<sqlx::Postgres>::new",
        &str_literal(&count),
    );
    super::search::push_scope(output, feature);
    push_q(output, searched);
    output.push_str("    query.build_query_scalar::<i64>().fetch_one(executor).await\n}\n");
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
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]\n",
    );
    push_to_schema(output, feature.openapi);
    output.push_str("#[serde(rename_all = \"snake_case\")]\n");
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
    output.push_str("        }\n    }\n");
    if feature.serves(Endpoint::Search) {
        output.push('\n');
        push_doc(
            output,
            4,
            &["The key `name` names, in `snake_case`, as a search body's `sort` does."],
        );
        output.push_str(
            "    #[must_use]\n    pub fn from_name(name: &str) -> Option<Self> {\n        match name {\n",
        );
        for column in &feature.sort {
            let variant = column.sort_variant();
            output.push_str(&format!(
                "            {} => Some(Self::{variant}),\n",
                str_literal(&super::search::serde_snake_case(&variant))
            ));
        }
        output.push_str("            _ => None,\n        }\n    }\n");
    }
    output.push_str("}\n");
}
