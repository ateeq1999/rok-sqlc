//! The shared `db/search.rs` runtime module every resource's search uses.

use crate::generator::openapi::TO_SCHEMA;
use crate::generator::{
    DeriveOptions, HEADER, push_derives, push_serde_snake_case, replace_once, serde_derives,
};

/// The `Ordered::Between` variant, which utoipa 5 can't describe as two
/// values of a generic type: with `openapi`, it is described as any value.
const BETWEEN: &str =
    "    /// Between the two values, inclusive on both ends.\n    Between(T, T),\n";

/// Renders the shared `db/search.rs`. With `serde`, the types an API layer
/// takes a search from (`Direction`, `Sort`, and the condition enums) derive
/// serde; `Page` and `Limit` do not, since deserializing would skip their
/// clamping. With `openapi`, the condition enums, which a search body
/// holds, derive `ToSchema`.
pub fn render_shared(derives: DeriveOptions) -> String {
    let serde = derives.serde;
    let mut output = format!("{HEADER}\n");
    for (part, next) in SHARED.iter().zip(SHARED.iter().skip(1)) {
        output.push_str(part);
        let mut extra = serde_derives(serde).to_vec();
        let condition = ["pub enum Ordered", "pub enum Equality", "pub enum Text"]
            .iter()
            .any(|head| next.starts_with(head));
        if derives.openapi && condition {
            extra.push(TO_SCHEMA);
        }
        push_derives(&mut output, &extra);
        if serde && next.starts_with("pub enum ") {
            push_serde_snake_case(&mut output);
        }
    }
    output.push_str(SHARED[SHARED.len() - 1]);
    if derives.openapi {
        let between = replace_once(
            BETWEEN,
            "    Between",
            "    #[schema(value_type = Value)]\n    Between",
        );
        output = replace_once(&output, BETWEEN, &between);
    }
    output
}

/// The text of `db/search.rs`, in parts: every part but the last ends with
/// the `#[derive(...)]` line of a type that derives serde with `serde`
/// (`Direction`, `Sort`, `Ordered`, `Equality`, and `Text`), so the serde
/// derives join exactly those lists.
const SHARED: [&str; 6] = [
    r#"//! Types shared by every resource's `search`: pages, sorting, and typed
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
"#,
    r#"pub enum Direction {
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
"#,
    r#"pub struct Sort<K> {
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
"#,
    r#"pub enum Ordered<T> {
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
        let shared = match self {
            Ordered::Eq(value) => Equality::Eq(value),
            Ordered::Ne(value) => Equality::Ne(value),
            Ordered::In(values) => Equality::In(values),
            Ordered::NotIn(values) => Equality::NotIn(values),
            Ordered::Lt(value) => return push_operator(query, " < ", value),
            Ordered::Le(value) => return push_operator(query, " <= ", value),
            Ordered::Gt(value) => return push_operator(query, " > ", value),
            Ordered::Ge(value) => return push_operator(query, " >= ", value),
            Ordered::Between(low, high) => {
                push_operator(query, " BETWEEN ", low);
                return push_operator(query, " AND ", high);
            }
        };
        shared.push_condition(query);
    }
}

/// A condition on a boolean or UUID column.
#[derive(Debug, Clone, PartialEq, Eq)]
"#,
    r#"pub enum Equality<T> {
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
        self.push_condition(query);
    }

    /// Appends the condition after its column. `Ordered` and `Text` share
    /// these four conditions and push them through here.
    fn push_condition(self, query: &mut QueryBuilder<Postgres>)
    where
        T: for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
        Vec<T>: for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres>,
    {
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
/// `table` is pushed as SQL text, not bound: only the generated filters
/// call this, with a table name rok-sqlc quoted. Never pass user input.
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
///
/// Every argument is pushed as SQL text, not bound: only the generated
/// filters call this, with aliases and column names rok-sqlc generated and
/// quoted. Never pass user input.
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
"#,
    r#"pub enum Text {
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
        let pattern = match self {
            Text::Eq(value) => return Equality::Eq(value).push_condition(query),
            Text::Ne(value) => return Equality::Ne(value).push_condition(query),
            Text::In(values) => return Equality::In(values).push_condition(query),
            Text::NotIn(values) => return Equality::NotIn(values).push_condition(query),
            Text::Contains(value) => format!("%{}%", escape_like(&value)),
            Text::StartsWith(value) => format!("{}%", escape_like(&value)),
            Text::EndsWith(value) => format!("%{}", escape_like(&value)),
        };
        push_operator(query, " LIKE ", pattern);
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
"#,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The template still holds [`BETWEEN`]: a patch that misses panics in a
    /// debug build.
    #[test]
    fn between_is_described_as_any_value() {
        let derives = DeriveOptions {
            serde: true,
            validate: false,
            openapi: true,
        };
        let text = render_shared(derives);
        assert!(text.contains("    #[schema(value_type = Value)]\n    Between(T, T),\n"));
    }
}
