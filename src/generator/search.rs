//! Typed filtering, allowlisted sorting, and pagination.
//!
//! `db/search.rs` holds the types every resource shares (`Page`, `Sort`, and
//! the per-type condition enums); each resource gets a `filter.rs` with its
//! `{Struct}Filter` and `{Struct}SortKey`, and `read.rs` gets `search`.
//! Conditions are rendered from schema column names only; every value is
//! bound.

use crate::schema::{Column, FilterKind, Table, mentions_generated_type};

use super::sql::{ident, str_literal};
use super::{HEADER, push_doc};

/// Which search functions a resource gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchApi {
    /// `search`: needs at least one sort key, or `Sort<XSortKey>` could
    /// never be constructed.
    pub search: bool,
    /// `search_after`: needs a key (other than the tenant column) to page by.
    pub search_after: bool,
}

impl SearchApi {
    pub fn for_table(table: &Table) -> Self {
        Self {
            search: !sortable(table).is_empty(),
            search_after: !table.key_columns().is_empty(),
        }
    }

    /// Whether the resource gets a `filter.rs` at all.
    pub fn any(self) -> bool {
        self.search || self.search_after
    }
}

impl FilterKind {
    /// The condition enum in `db/search.rs`.
    fn type_name(self) -> &'static str {
        match self {
            FilterKind::Ordered => "Ordered",
            FilterKind::Text => "Text",
            FilterKind::Equality => "Equality",
        }
    }

    fn condition_type(self, column: &Column) -> String {
        match self {
            FilterKind::Text => self.type_name().to_owned(),
            _ => format!("{}<{}>", self.type_name(), column.value_type()),
        }
    }
}

/// Appends `callee(args);` at `indent`, one argument per line when the
/// arguments exceed rustfmt's `fn_call_width` (60) or the line its
/// `max_width` (100).
fn push_call(output: &mut String, indent: usize, callee: &str, args: &[String]) {
    let pad = " ".repeat(indent);
    let joined = args.join(", ");
    let one_line = format!("{pad}{callee}({joined});");
    if joined.len() <= 60 && one_line.len() <= super::MAX_WIDTH {
        output.push_str(&one_line);
        output.push('\n');
        return;
    }
    output.push_str(&format!("{pad}{callee}(\n"));
    for arg in args {
        output.push_str(&format!("{pad}    {arg},\n"));
    }
    output.push_str(&format!("{pad});\n"));
}

/// `{ items }` for a struct, enum, or match body: `{}` when empty, as rustfmt
/// writes it.
fn push_block(output: &mut String, head: &str, items: &[String], indent: usize) {
    let pad = " ".repeat(indent);
    if items.is_empty() {
        output.push_str(&format!("{pad}{head} {{}}\n"));
        return;
    }
    output.push_str(&format!("{pad}{head} {{\n"));
    for item in items {
        output.push_str(&format!("{pad}    {item}\n"));
    }
    output.push_str(&format!("{pad}}}\n"));
}

/// Columns `search` may order by: every filterable column.
pub fn sortable(table: &Table) -> Vec<&Column> {
    table
        .columns
        .iter()
        .filter(|column| column.filter_kind().is_some())
        .collect()
}

/// Renders the resource's `filter.rs`, or `None` when the resource has no
/// search function to use it.
pub fn render_filter(table: &Table) -> Option<String> {
    let api = SearchApi::for_table(table);
    if !api.any() {
        return None;
    }
    let filter = format!("{}Filter", table.struct_name);
    let sort_key = format!("{}SortKey", table.struct_name);
    let filterable: Vec<(&Column, FilterKind)> = table
        .columns
        .iter()
        .filter_map(|column| column.filter_kind().map(|kind| (column, kind)))
        .collect();

    let mut output = format!("{HEADER}\n");
    let has_nullable = table.columns.iter().any(|column| column.nullable);
    let mut imports: Vec<&str> = filterable
        .iter()
        .map(|(_, kind)| kind.type_name())
        .collect();
    if has_nullable {
        imports.push("push_null_check");
    }
    imports.sort_unstable();
    imports.dedup();
    let mut paths = match imports.as_slice() {
        [] => Vec::new(),
        [single] => vec![format!("super::super::search::{single}")],
        many => vec![format!("super::super::search::{{{}}}", many.join(", "))],
    };
    // Typed ids appear in `Ordered<_>`/`Equality<_>` conditions only.
    paths.extend(super::keys::id_imports(
        table,
        filterable
            .iter()
            .filter(|(_, kind)| *kind != FilterKind::Text)
            .map(|(column, _)| *column),
        false,
    ));
    if filterable
        .iter()
        .any(|(column, _)| mentions_generated_type(column.value_type()))
    {
        paths.push("super::super::types".to_owned());
    }
    if !paths.is_empty() {
        output.push_str(&super::use_paths(&paths));
        output.push('\n');
    }
    let title = format!("Conditions for `search` over `{}`.", table.display_name());
    let mut doc = vec![
        title.as_str(),
        "",
        "Every condition in every field must hold (they are combined with `AND`);",
        "an empty filter matches every row. A nullable column's `*_is_null` field",
        "selects rows where it is (`Some(true)`) or is not (`Some(false)`) `NULL`.",
    ];
    let live = super::sql::live_condition(table);
    if live.is_some() {
        doc.extend([
            "Soft-deleted rows match only when `include_deleted` is set; an empty filter",
            "matches every live row.",
        ]);
    }
    push_doc(&mut output, 0, &doc);
    output.push_str("#[derive(Debug, Clone, Default, PartialEq)]\n");
    let mut fields = Vec::new();
    if live.is_some() {
        fields.push(format!(
            "pub {}: bool,",
            super::conventions::INCLUDE_DELETED
        ));
    }
    for column in &table.columns {
        if let Some(kind) = column.filter_kind() {
            fields.push(format!(
                "pub {}: Vec<{}>,",
                column.rust_name,
                kind.condition_type(column)
            ));
        }
        if column.nullable {
            fields.push(format!("pub {}: Option<bool>,", column.null_check_field()));
        }
    }
    push_block(&mut output, &format!("pub struct {filter}"), &fields, 0);
    output.push('\n');

    output.push_str(&format!("impl {filter} {{\n"));
    push_doc(
        &mut output,
        4,
        &["Appends the `WHERE` clause for these conditions."],
    );
    output.push_str(
        "    pub(crate) fn push_where(self, query: &mut sqlx::QueryBuilder<sqlx::Postgres>) {\n",
    );
    output.push_str("        query.push(\" WHERE TRUE\");\n");
    if let Some(live) = &live {
        output.push_str(&format!(
            "        if !self.{} {{\n            query.push({});\n        }}\n",
            super::conventions::INCLUDE_DELETED,
            str_literal(&format!(" AND {live}"))
        ));
    }
    for column in &table.columns {
        let name = str_literal(&ident(&column.sql_name));
        if column.filter_kind().is_some() {
            output.push_str(&format!(
                "        for condition in self.{} {{\n",
                column.rust_name
            ));
            push_call(
                &mut output,
                12,
                "condition.push",
                &[name.clone(), "query".to_owned()],
            );
            output.push_str("        }\n");
        }
        if column.nullable {
            push_call(
                &mut output,
                8,
                "push_null_check",
                &[
                    name,
                    format!("self.{}", column.null_check_field()),
                    "query".to_owned(),
                ],
            );
        }
    }
    output.push_str("    }\n}\n");
    if !api.search {
        return Some(output);
    }

    output.push('\n');
    push_doc(&mut output, 0, &["Columns `search` may sort by."]);
    output.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n");
    let sortable = sortable(table);
    let variants: Vec<String> = sortable
        .iter()
        .map(|column| format!("{},", column.sort_variant()))
        .collect();
    push_block(&mut output, &format!("pub enum {sort_key}"), &variants, 0);
    output.push('\n');
    output.push_str(&format!("impl {sort_key} {{\n"));
    push_doc(&mut output, 4, &["The SQL column this key sorts by."]);
    output.push_str("    pub fn column(self) -> &'static str {\n");
    let arms: Vec<String> = sortable
        .iter()
        .map(|column| {
            format!(
                "Self::{} => {},",
                column.sort_variant(),
                str_literal(&ident(&column.sql_name))
            )
        })
        .collect();
    push_block(&mut output, "match self", &arms, 8);
    output.push_str("    }\n}\n");
    Some(output)
}

/// Renders the shared `db/search.rs`.
pub fn render_shared() -> String {
    format!("{HEADER}\n{SHARED}")
}

const SHARED: &str = r#"//! Types shared by every resource's `search`: pages, sorting, and typed
//! filter conditions.

use sqlx::{Postgres, QueryBuilder};

/// The most rows a single page may hold; `Page::new` clamps larger limits.
pub const MAX_PAGE_SIZE: i64 = 500;

/// How many rows one request may return: always within `1..=MAX_PAGE_SIZE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit(i64);

impl Limit {
    /// Clamps `rows` to `1..=MAX_PAGE_SIZE`.
    pub fn new(rows: i64) -> Self {
        Self(rows.clamp(1, MAX_PAGE_SIZE))
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

/// A window of results: at most `limit` rows after skipping `offset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    limit: Limit,
    offset: i64,
}

impl Page {
    /// Clamps `limit` to `1..=MAX_PAGE_SIZE` and `offset` to zero or more.
    pub fn new(limit: i64, offset: i64) -> Self {
        Self {
            limit: Limit::new(limit),
            offset: offset.max(0),
        }
    }

    /// The first page of `limit` rows.
    pub fn first(limit: i64) -> Self {
        Self::new(limit, 0)
    }

    /// The page that follows this one.
    pub fn next(self) -> Self {
        Self::new(self.limit(), self.offset.saturating_add(self.limit()))
    }

    pub fn limit(self) -> i64 {
        self.limit.get()
    }

    pub fn offset(self) -> i64 {
        self.offset
    }
}

/// Sort direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Asc,
    Desc,
}

impl Direction {
    /// The SQL keyword, with a leading space.
    pub fn sql(self) -> &'static str {
        match self {
            Direction::Asc => " ASC",
            Direction::Desc => " DESC",
        }
    }
}

/// Sorts by one allowlisted key; `search` breaks ties by primary key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort<K> {
    pub key: K,
    pub direction: Direction,
}

impl<K> Sort<K> {
    pub fn asc(key: K) -> Self {
        Self {
            key,
            direction: Direction::Asc,
        }
    }

    pub fn desc(key: K) -> Self {
        Self {
            key,
            direction: Direction::Desc,
        }
    }
}

/// A condition on a number, decimal, date, or time column.
#[derive(Debug, Clone, PartialEq)]
pub enum Ordered<T> {
    Eq(T),
    Ne(T),
    Lt(T),
    Le(T),
    Gt(T),
    Ge(T),
    /// Inclusive on both ends.
    Between(T, T),
    In(Vec<T>),
    NotIn(Vec<T>),
}

impl<T> Ordered<T> {
    /// Appends ` AND <column> <condition>`, binding every value.
    #[doc(hidden)]
    pub fn push(self, column: &str, query: &mut QueryBuilder<Postgres>)
    where
        T: for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
        Vec<T>: for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
    {
        query.push(" AND ");
        query.push(column);
        match self {
            Ordered::Eq(value) => push_operator(query, " = ", value),
            Ordered::Ne(value) => push_operator(query, " <> ", value),
            Ordered::Lt(value) => push_operator(query, " < ", value),
            Ordered::Le(value) => push_operator(query, " <= ", value),
            Ordered::Gt(value) => push_operator(query, " > ", value),
            Ordered::Ge(value) => push_operator(query, " >= ", value),
            Ordered::Between(low, high) => {
                push_operator(query, " BETWEEN ", low);
                push_operator(query, " AND ", high);
            }
            Ordered::In(values) => push_set(query, " = ANY(", values),
            Ordered::NotIn(values) => push_set(query, " <> ALL(", values),
        }
    }
}

/// A condition on a boolean or UUID column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Equality<T> {
    Eq(T),
    Ne(T),
    In(Vec<T>),
    NotIn(Vec<T>),
}

impl<T> Equality<T> {
    /// Appends ` AND <column> <condition>`, binding every value.
    #[doc(hidden)]
    pub fn push(self, column: &str, query: &mut QueryBuilder<Postgres>)
    where
        T: for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
        Vec<T>: for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
    {
        query.push(" AND ");
        query.push(column);
        match self {
            Equality::Eq(value) => push_operator(query, " = ", value),
            Equality::Ne(value) => push_operator(query, " <> ", value),
            Equality::In(values) => push_set(query, " = ANY(", values),
            Equality::NotIn(values) => push_set(query, " <> ALL(", values),
        }
    }
}

/// Appends ` AND <column> IS [NOT] NULL` for `Some(true)` / `Some(false)`.
#[doc(hidden)]
pub fn push_null_check(column: &str, is_null: Option<bool>, query: &mut QueryBuilder<Postgres>) {
    match is_null {
        Some(true) => {
            query.push(" AND ");
            query.push(column);
            query.push(" IS NULL");
        }
        Some(false) => {
            query.push(" AND ");
            query.push(column);
            query.push(" IS NOT NULL");
        }
        None => {}
    }
}

/// A condition on a text column. The substring variants match their
/// argument literally: `%`, `_`, and `\` in it are escaped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Text {
    Eq(String),
    Ne(String),
    In(Vec<String>),
    NotIn(Vec<String>),
    Contains(String),
    StartsWith(String),
    EndsWith(String),
}

impl Text {
    /// Appends ` AND <column> <condition>`, binding every value.
    #[doc(hidden)]
    pub fn push(self, column: &str, query: &mut QueryBuilder<Postgres>) {
        query.push(" AND ");
        query.push(column);
        match self {
            Text::Eq(value) => push_operator(query, " = ", value),
            Text::Ne(value) => push_operator(query, " <> ", value),
            Text::In(values) => push_set(query, " = ANY(", values),
            Text::NotIn(values) => push_set(query, " <> ALL(", values),
            Text::Contains(value) => {
                push_operator(query, " LIKE ", format!("%{}%", escape_like(&value)));
            }
            Text::StartsWith(value) => {
                push_operator(query, " LIKE ", format!("{}%", escape_like(&value)));
            }
            Text::EndsWith(value) => {
                push_operator(query, " LIKE ", format!("%{}", escape_like(&value)));
            }
        }
    }
}

/// Escapes `LIKE` wildcards using PostgreSQL's default escape character.
fn escape_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn push_operator<T>(query: &mut QueryBuilder<Postgres>, operator: &str, value: T)
where
    T: for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
{
    query.push(operator);
    query.push_bind(value);
}

fn push_set<T>(query: &mut QueryBuilder<Postgres>, operator: &str, values: Vec<T>)
where
    Vec<T>: for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
{
    query.push(operator);
    query.push_bind(values);
    query.push(")");
}
"#;

#[cfg(test)]
mod tests {
    use super::super::test_support::{column, table};
    use super::*;

    /// SQLx 0.9's `QueryBuilder` takes no lifetime parameter.
    #[test]
    fn search_code_names_query_builder_without_a_lifetime() {
        let filter = render_filter(&table(vec![column("id", "i64")], &["id"]))
            .expect("an i64 column is filterable");
        for rendered in [render_shared(), filter] {
            assert!(rendered.contains("QueryBuilder<"), "{rendered}");
            assert!(!rendered.contains("QueryBuilder<'"), "{rendered}");
        }
    }
}
