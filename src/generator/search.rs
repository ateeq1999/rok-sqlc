//! Typed filtering, allowlisted sorting, and pagination.
//!
//! `db/search.rs` holds the types every resource shares (`Page`, `Sort`, and
//! the per-type condition enums); each resource gets a `filter.rs` with its
//! `{Struct}Filter` and `{Struct}SortKey`, and `read.rs` gets `search`.
//! Conditions are rendered from schema column names only; every value is
//! bound.
//!
//! A resource with relationships (ADR 0014) also gets a filter field per
//! relationship it can follow, holding the related resource's filter. It is
//! rendered as an `EXISTS` subquery over the related table under an alias
//! (`_r1`, `_r2`, … by nesting depth), correlated with the enclosing row, so
//! rows never repeat. The related table's soft-delete condition always
//! applies inside it, and a related tenant table is correlated on the tenant
//! column with the enclosing row, which is itself scoped to the tenant.

use crate::schema::{Column, FilterKind, Relationship, Table, mentions_generated_type};

use super::sql::{ident, live_condition, str_literal, table_ref};
use super::{HEADER, doc_name, push_doc};

/// A relationship `{Struct}Filter` can follow, resolved against its target.
pub struct FilterRelationship<'a> {
    pub relationship: &'a Relationship,
    pub target: &'a Table,
}

/// The relationships `table`'s filter gets a field for: those whose target
/// has a filter that can be nested (it has relationships of its own, as
/// every target has the inverse). A tenant table is left out when `table`
/// has no tenant column: there is no tenant to scope its rows by.
pub fn filter_relationships<'a>(
    table: &'a Table,
    tables: &'a [Table],
) -> Vec<FilterRelationship<'a>> {
    if !SearchApi::for_table(table).any() {
        return Vec::new();
    }
    table
        .relationships
        .iter()
        .filter_map(|relationship| {
            let target = tables
                .iter()
                .find(|other| other.module_name == relationship.target)?;
            let nestable = SearchApi::for_table(target).any() && !target.relationships.is_empty();
            let scoped = target.tenant().is_none() || table.tenant().is_some();
            (nestable && scoped).then_some(FilterRelationship {
                relationship,
                target,
            })
        })
        .collect()
}

/// Fails when a relationship filter field of `table` would be named like
/// one of its column fields or its `include_deleted` switch.
pub fn check_filter_fields(table: &Table, tables: &[Table]) -> Result<(), String> {
    let relationships = filter_relationships(table, tables);
    if relationships.is_empty() {
        return Ok(());
    }
    let mut fields: Vec<(String, String)> = Vec::new();
    if live_condition(table).is_some() {
        let name = super::conventions::INCLUDE_DELETED;
        fields.push((name.to_owned(), format!("`{name}` switch")));
    }
    for column in &table.columns {
        if column.filter_kind().is_some() {
            fields.push((
                column.rust_name.clone(),
                format!(
                    "`{}` field for column `{}`",
                    column.rust_name, column.sql_name
                ),
            ));
        }
        if column.nullable {
            let name = column.null_check_field();
            let described = format!("`{name}` field for column `{}`", column.sql_name);
            fields.push((name, described));
        }
    }
    for FilterRelationship { relationship, .. } in relationships {
        if let Some((_, field)) = fields.iter().find(|(name, _)| *name == relationship.name) {
            return Err(format!(
                "the `{}` relationship of `{}` collides with the search filter's {field}",
                relationship.name,
                table.display_name()
            ));
        }
    }
    Ok(())
}

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
    push_call_ending(output, indent, callee, args, ";");
}

/// `push_call`, ending the call with `end` (`;`, `?;`, or nothing).
fn push_call_ending(output: &mut String, indent: usize, callee: &str, args: &[String], end: &str) {
    let pad = " ".repeat(indent);
    let joined = args.join(", ");
    let one_line = format!("{pad}{callee}({joined}){end}");
    if joined.len() <= 60 && one_line.len() <= super::MAX_WIDTH {
        output.push_str(&one_line);
        output.push('\n');
        return;
    }
    output.push_str(&format!("{pad}{callee}(\n"));
    for arg in args {
        output.push_str(&format!("{pad}    {arg},\n"));
    }
    output.push_str(&format!("{pad}){end}\n"));
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

/// The `query` parameter every condition-pushing function takes.
const QUERY_PARAM: &str = "query: &mut sqlx::QueryBuilder<sqlx::Postgres>";

/// Renders the resource's `filter.rs`, or `None` when the resource has no
/// search function to use it. `tables` holds every table generated with it.
pub fn render_filter(table: &Table, tables: &[Table]) -> Option<String> {
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
    let relationships = filter_relationships(table, tables);
    // A table with relationships may be the target of another's filter, so
    // its conditions get their own function, `push_conditions`.
    let split = !table.relationships.is_empty();
    let fallible = !relationships.is_empty();

    let mut output = format!("{HEADER}\n");
    let has_nullable = table.columns.iter().any(|column| column.nullable);
    let mut imports: Vec<&str> = filterable
        .iter()
        .map(|(_, kind)| kind.type_name())
        .collect();
    if has_nullable {
        imports.push("push_null_check");
    }
    if fallible {
        imports.extend(["correlate", "open_exists"]);
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
    for FilterRelationship { target, .. } in &relationships {
        if target.module_name != table.module_name {
            paths.push(format!(
                "super::super::{}::filter::{}Filter",
                target.module_name, target.struct_name
            ));
        }
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
    let live = live_condition(table);
    if live.is_some() {
        doc.extend([
            "Soft-deleted rows match only when `include_deleted` is set; an empty filter",
            "matches every live row.",
        ]);
    }
    if fallible {
        doc.extend([
            "",
            "A relationship field matches rows with a related row matching its filter,",
            "through an `EXISTS` subquery, so no row is returned twice; for a to-many",
            "relationship, at least one related row. An empty related filter only",
            "requires a related row: a `NULL` foreign key has none. Soft-deleted related",
            "rows never match (`include_deleted` applies to the searched table only),",
            "and a related tenant table's rows match only within the searched row's",
            "tenant. Relationship filters nest at most `MAX_RELATIONSHIP_DEPTH` deep.",
        ]);
    }
    push_doc(&mut output, 0, &doc);
    output.push_str("#[derive(Debug, Clone, Default, PartialEq)]\n");
    let mut fields = Vec::new();
    if live.is_some() {
        fields.push("/// Whether soft-deleted rows match too.".to_owned());
        fields.push(format!(
            "pub {}: bool,",
            super::conventions::INCLUDE_DELETED
        ));
    }
    for column in &table.columns {
        let name = doc_name(&column.sql_name);
        if let Some(kind) = column.filter_kind() {
            fields.push(format!("/// Conditions on the {name} column."));
            fields.push(format!(
                "pub {}: Vec<{}>,",
                column.rust_name,
                kind.condition_type(column)
            ));
        }
        if column.nullable {
            fields.push(format!("/// Whether the {name} column is `NULL`."));
            fields.push(format!("pub {}: Option<bool>,", column.null_check_field()));
        }
    }
    for FilterRelationship {
        relationship,
        target,
    } in &relationships
    {
        let name = &relationship.name;
        let target_name = doc_name(&target.display_name());
        fields.push(if relationship.kind.is_to_many() {
            format!("/// Matches rows with at least one of their `{name}` ({target_name} rows) matching this filter.")
        } else {
            format!("/// Matches rows whose `{name}` ({target_name} row) exists and matches this filter.")
        });
        fields.push(format!(
            "pub {name}: Option<Box<{}Filter>>,",
            target.struct_name
        ));
    }
    push_block(&mut output, &format!("pub struct {filter}"), &fields, 0);
    output.push('\n');

    output.push_str(&format!("impl {filter} {{\n"));
    let mut where_doc = vec!["Appends the `WHERE` clause for these conditions."];
    if fallible {
        where_doc.extend(ERRORS_DOC);
        push_doc(&mut output, 4, &where_doc);
        output.push_str(&format!(
            "    pub(crate) fn push_where(\n        self,\n        {QUERY_PARAM},\n    ) -> Result<(), sqlx::Error> {{\n"
        ));
    } else {
        push_doc(&mut output, 4, &where_doc);
        output.push_str(&format!(
            "    pub(crate) fn push_where(self, {QUERY_PARAM}) {{\n"
        ));
    }
    output.push_str("        query.push(\" WHERE TRUE\");\n");
    if let Some(live) = &live {
        output.push_str(&format!(
            "        if !self.{} {{\n            query.push({});\n        }}\n",
            super::conventions::INCLUDE_DELETED,
            str_literal(&format!(" AND {live}"))
        ));
    }
    if !split {
        push_column_conditions(&mut output, table);
        output.push_str("    }\n}\n");
    } else {
        if fallible {
            push_call_ending(
                &mut output,
                8,
                "self.push_conditions",
                &[
                    str_literal(&ident(&table.sql_name)),
                    "0".to_owned(),
                    "query".to_owned(),
                ],
                "",
            );
        } else {
            output.push_str("        self.push_conditions(query);\n");
        }
        output.push_str("    }\n\n");
        push_conditions_fn(&mut output, table, tables, &relationships);
        output.push_str("}\n");
    }
    if !api.search {
        return Some(output);
    }

    output.push('\n');
    push_doc(&mut output, 0, &["Columns `search` may sort by."]);
    output.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n");
    let sortable = sortable(table);
    let variants: Vec<String> = sortable
        .iter()
        .flat_map(|column| {
            [
                format!("/// Sorts by the {} column.", doc_name(&column.sql_name)),
                format!("{},", column.sort_variant()),
            ]
        })
        .collect();
    push_block(&mut output, &format!("pub enum {sort_key}"), &variants, 0);
    output.push('\n');
    output.push_str(&format!("impl {sort_key} {{\n"));
    push_doc(&mut output, 4, &["The SQL column this key sorts by."]);
    output.push_str("    #[must_use]\n    pub fn column(self) -> &'static str {\n");
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

/// The `# Errors` section of a function that follows relationship filters.
const ERRORS_DOC: [&str; 5] = [
    "",
    "# Errors",
    "",
    "`sqlx::Error::InvalidArgument` when relationship filters nest more than",
    "`MAX_RELATIONSHIP_DEPTH` deep.",
];

/// Appends ` AND <column> <condition>` for every column condition of the
/// filter, inside a function taking `self` and `query`.
fn push_column_conditions(output: &mut String, table: &Table) {
    for column in &table.columns {
        let name = str_literal(&ident(&column.sql_name));
        if column.filter_kind().is_some() {
            output.push_str(&format!(
                "        for condition in self.{} {{\n",
                column.rust_name
            ));
            push_call(
                output,
                12,
                "condition.push",
                &[name.clone(), "query".to_owned()],
            );
            output.push_str("        }\n");
        }
        if column.nullable {
            push_call(
                output,
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
}

/// `push_conditions`: the column conditions, then an `EXISTS` subquery per
/// set relationship field. It takes the enclosing row's table or alias
/// (`outer`) and the nesting depth when the filter has relationship fields.
fn push_conditions_fn(
    output: &mut String,
    table: &Table,
    tables: &[Table],
    relationships: &[FilterRelationship<'_>],
) {
    let mut doc = vec![
        "Appends ` AND <condition>` for each condition, without the `WHERE` clause or",
        "the soft-delete condition, so another resource's filter can nest this one.",
    ];
    if relationships.is_empty() {
        push_doc(output, 4, &doc);
        output.push_str(&format!(
            "    pub(crate) fn push_conditions(self, {QUERY_PARAM}) {{\n"
        ));
        push_column_conditions(output, table);
        output.push_str("    }\n");
        return;
    }
    doc.extend([
        "`outer` names the row being filtered (its table, or a relationship alias)",
        "and `depth` counts the relationship filters this one is nested in.",
    ]);
    doc.extend(ERRORS_DOC);
    push_doc(output, 4, &doc);
    output.push_str(&format!(
        "    pub(crate) fn push_conditions(\n        self,\n        outer: &str,\n        depth: usize,\n        {QUERY_PARAM},\n    ) -> Result<(), sqlx::Error> {{\n"
    ));
    push_column_conditions(output, table);
    let quoted = |name: &str| str_literal(&ident(name));
    for FilterRelationship {
        relationship,
        target,
    } in relationships
    {
        output.push_str(&format!(
            "        if let Some(filter) = self.{} {{\n",
            relationship.name
        ));
        push_call_ending(
            output,
            12,
            "let alias = open_exists",
            &[
                str_literal(&table_ref(target)),
                "depth".to_owned(),
                "query".to_owned(),
            ],
            "?;",
        );
        let mut pairs = vec![(&relationship.target_column, &relationship.column)];
        // `filter_relationships` keeps a tenant target only when this table
        // has a tenant column too; both use the configured column name.
        pairs.extend(
            target
                .tenant()
                .zip(table.tenant())
                .map(|(theirs, ours)| (&theirs.sql_name, &ours.sql_name)),
        );
        for (theirs, ours) in pairs {
            push_call(
                output,
                12,
                "correlate",
                &[
                    "&alias".to_owned(),
                    quoted(theirs),
                    "outer".to_owned(),
                    quoted(ours),
                    "query".to_owned(),
                ],
            );
        }
        if let Some(live) = live_condition(target) {
            output.push_str(&format!(
                "            query.push({});\n",
                str_literal(&format!(" AND {live}"))
            ));
        }
        if filter_relationships(target, tables).is_empty() {
            output.push_str("            filter.push_conditions(query);\n");
        } else {
            output.push_str("            filter.push_conditions(&alias, depth + 1, query)?;\n");
        }
        output.push_str("            query.push(\")\");\n        }\n");
    }
    output.push_str("        Ok(())\n    }\n");
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
    #[must_use]
    pub fn new(rows: i64) -> Self {
        Self(rows.clamp(1, MAX_PAGE_SIZE))
    }

    /// The number of rows.
    #[must_use]
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
    #[must_use]
    pub fn new(limit: i64, offset: i64) -> Self {
        Self {
            limit: Limit::new(limit),
            offset: offset.max(0),
        }
    }

    /// The first page of `limit` rows.
    #[must_use]
    pub fn first(limit: i64) -> Self {
        Self::new(limit, 0)
    }

    /// The page that follows this one.
    #[must_use]
    pub fn next(self) -> Self {
        Self::new(self.limit(), self.offset.saturating_add(self.limit()))
    }

    /// The most rows the page holds.
    #[must_use]
    pub fn limit(self) -> i64 {
        self.limit.get()
    }

    /// The number of rows skipped before the page.
    #[must_use]
    pub fn offset(self) -> i64 {
        self.offset
    }
}

/// Sort direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Direction {
    /// Ascending: smallest first.
    #[default]
    Asc,
    /// Descending: largest first.
    Desc,
}

impl Direction {
    /// The SQL keyword, with a leading space.
    #[must_use]
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
    /// The column to sort by.
    pub key: K,
    /// Ascending or descending.
    pub direction: Direction,
}

impl<K> Sort<K> {
    /// Sorts by `key`, smallest first.
    pub fn asc(key: K) -> Self {
        Self {
            key,
            direction: Direction::Asc,
        }
    }

    /// Sorts by `key`, largest first.
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
    /// Equal to the value.
    Eq(T),
    /// Not equal to the value.
    Ne(T),
    /// Less than the value.
    Lt(T),
    /// Less than or equal to the value.
    Le(T),
    /// Greater than the value.
    Gt(T),
    /// Greater than or equal to the value.
    Ge(T),
    /// Between the two values, inclusive on both ends.
    Between(T, T),
    /// Equal to one of the values.
    In(Vec<T>),
    /// Equal to none of the values.
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
    /// Equal to the value.
    Eq(T),
    /// Not equal to the value.
    Ne(T),
    /// Equal to one of the values.
    In(Vec<T>),
    /// Equal to none of the values.
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

/// How deeply relationship filters may nest: a filter's relationship field
/// counts one, a relationship field inside that filter two, and so on.
/// `search` fails with `sqlx::Error::InvalidArgument` past this depth.
pub const MAX_RELATIONSHIP_DEPTH: usize = 4;

/// Appends ` AND EXISTS (SELECT 1 FROM <table> AS <alias> WHERE TRUE` for a
/// relationship filter nested in `depth` others, returning the alias. The
/// caller appends the correlation and conditions, then `)`.
///
/// # Errors
///
/// `sqlx::Error::InvalidArgument` when `depth` reaches
/// `MAX_RELATIONSHIP_DEPTH`.
#[doc(hidden)]
pub fn open_exists(
    table: &str,
    depth: usize,
    query: &mut QueryBuilder<Postgres>,
) -> Result<String, sqlx::Error> {
    if depth >= MAX_RELATIONSHIP_DEPTH {
        return Err(sqlx::Error::InvalidArgument(format!(
            "relationship filters nest more than {MAX_RELATIONSHIP_DEPTH} deep"
        )));
    }
    let alias = format!("_r{}", depth + 1);
    query.push(" AND EXISTS (SELECT 1 FROM ");
    query.push(table);
    query.push(" AS ");
    query.push(&alias);
    query.push(" WHERE TRUE");
    Ok(alias)
}

/// Appends ` AND <alias>.<column> = <outer>.<outer_column>`.
#[doc(hidden)]
pub fn correlate(
    alias: &str,
    column: &str,
    outer: &str,
    outer_column: &str,
    query: &mut QueryBuilder<Postgres>,
) {
    query.push(" AND ");
    query.push(alias);
    query.push(".");
    query.push(column);
    query.push(" = ");
    query.push(outer);
    query.push(".");
    query.push(outer_column);
}

/// A condition on a text column. The substring variants match their
/// argument literally: `%`, `_`, and `\` in it are escaped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Text {
    /// Equal to the text.
    Eq(String),
    /// Not equal to the text.
    Ne(String),
    /// Equal to one of the texts.
    In(Vec<String>),
    /// Equal to none of the texts.
    NotIn(Vec<String>),
    /// Contains the text.
    Contains(String),
    /// Starts with the text.
    StartsWith(String),
    /// Ends with the text.
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

/// Escapes `LIKE` wildcards with the default escape character, `\`.
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
        let filter = render_filter(&table(vec![column("id", "i64")], &["id"]), &[])
            .expect("an i64 column is filterable");
        for rendered in [render_shared(), filter] {
            assert!(rendered.contains("QueryBuilder<"), "{rendered}");
            assert!(!rendered.contains("QueryBuilder<'"), "{rendered}");
        }
    }
}
