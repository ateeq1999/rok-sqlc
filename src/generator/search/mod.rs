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
mod conditions;
mod filter;
mod runtime;

use std::collections::HashMap;

use crate::schema::{Column, FilterKind, Relationship, Table};

use super::relationships;
use super::sql::live_condition;

pub use self::filter::render_filter;
pub use self::runtime::render_shared;

/// A relationship `{Struct}Filter` can follow, resolved against its target.
pub struct FilterRelationship<'a> {
    pub relationship: &'a Relationship,
    pub target: &'a Table,
    /// A many-to-many relationship's join table.
    pub join: Option<&'a Table>,
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
            let resolved = relationships::resolve(table, relationship, tables);
            let target = resolved.target;
            let join = resolved.join.map(|join| join.table);
            let nestable = SearchApi::for_table(target).any() && !target.relationships.is_empty();
            // A many-to-many also needs the tenant to scope its join rows.
            let scoped = std::iter::once(target)
                .chain(join)
                .all(|related| related.tenant().is_none() || table.tenant().is_some());
            (nestable && scoped).then_some(FilterRelationship {
                relationship,
                target,
                join,
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
    if live_condition(table, None).is_some() {
        let name = super::conventions::INCLUDE_DELETED;
        fields.push((name.to_owned(), format!("`{name}` switch")));
    }
    for column in filter_columns(table) {
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
    pub(crate) fn type_name(self) -> &'static str {
        match self {
            FilterKind::Ordered => "Ordered",
            FilterKind::Text => "Text",
            FilterKind::Equality => "Equality",
        }
    }

    pub(crate) fn condition_type(self, column: &Column) -> String {
        match self {
            FilterKind::Text => self.type_name().to_owned(),
            _ => format!("{}<{}>", self.type_name(), column.value_type()),
        }
    }
}

/// Appends `callee(args);` at `indent`, one argument per line when the
/// arguments exceed rustfmt's `fn_call_width` ([`FN_CALL_WIDTH`](super::FN_CALL_WIDTH)) or
/// the line its `max_width` ([`MAX_WIDTH`](super::MAX_WIDTH)).
fn push_call(output: &mut String, indent: usize, callee: &str, args: &[String]) {
    push_call_ending(output, indent, callee, args, ";");
}

/// `push_call`, ending the call with `end` (`;`, `?;`, or nothing).
fn push_call_ending(output: &mut String, indent: usize, callee: &str, args: &[String], end: &str) {
    let pad = " ".repeat(indent);
    let joined = args.join(", ");
    let one_line = format!("{pad}{callee}({joined}){end}");
    if joined.len() <= super::FN_CALL_WIDTH && one_line.len() <= super::MAX_WIDTH {
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

/// Columns the filter may have fields for: every column but the tenant
/// column, which the tenant parameter already scopes `search` by.
pub fn filter_columns(table: &Table) -> impl Iterator<Item = &Column> {
    table
        .columns
        .iter()
        .filter(|column| !table.is_tenant_column(column))
}

/// Why `table` cannot be searched, if its generated names would collide: a
/// nullable column's `x_is_null` filter field against another field, or two
/// columns mapping to the same `XSortKey` variant. `table` has its
/// conventions applied: the tenant column gets neither name, so it takes
/// part in no collision.
pub fn name_clash(table: &Table) -> Option<String> {
    let mut fields: HashMap<String, &str> = filter_columns(table)
        .map(|column| (column.rust_name.clone(), column.sql_name.as_str()))
        .collect();
    for column in filter_columns(table).filter(|column| column.nullable) {
        let field = column.null_check_field();
        if let Some(other) = fields.insert(field.clone(), &column.sql_name) {
            return Some(format!(
                "column `{}` needs the null-check filter field `{field}`, which column `{other}` already uses",
                column.sql_name
            ));
        }
    }

    let mut variants: HashMap<String, &str> = HashMap::new();
    for column in sortable(table) {
        let variant = column.sort_variant();
        if let Some(other) = variants.insert(variant.clone(), &column.sql_name) {
            return Some(format!(
                "columns `{other}` and `{}` both map to the sort key variant `{variant}`",
                column.sql_name
            ));
        }
    }
    None
}

/// Columns `search` may order by: every filterable column but the tenant
/// column.
pub fn sortable(table: &Table) -> Vec<&Column> {
    filter_columns(table)
        .filter(|column| column.filter_kind().is_some())
        .collect()
}

/// The `query` parameter every condition-pushing function takes.
const QUERY_PARAM: &str = "query: &mut sqlx::QueryBuilder<sqlx::Postgres>";

#[cfg(test)]
mod tests {
    use super::super::DeriveOptions;
    use super::super::test_support::{column, table};
    use super::*;

    /// SQLx 0.9's `QueryBuilder` takes no lifetime parameter.
    #[test]
    fn search_code_names_query_builder_without_a_lifetime() {
        let filter = render_filter(&table(vec![column("id", "i64")], &["id"]), &[], false)
            .expect("an i64 column is filterable");
        for rendered in [render_shared(DeriveOptions::default()), filter] {
            assert!(rendered.contains("QueryBuilder<"), "{rendered}");
            assert!(!rendered.contains("QueryBuilder<'"), "{rendered}");
        }
    }

    #[test]
    fn serde_derives_filters_and_sort_keys_on_request() {
        let users = table(vec![column("id", "i64")], &["id"]);
        assert!(!render_filter(&users, &[], false).unwrap().contains("serde"));
        let rendered = render_filter(&users, &[], true).unwrap();
        assert!(rendered.contains(
            "#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]\n#[serde(default)]\npub struct UserFilter {"
        ), "{rendered}");
        assert!(rendered.contains(
            "#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]\n#[serde(rename_all = \"snake_case\")]\npub enum UserSortKey {"
        ), "{rendered}");
    }

    #[test]
    fn serde_derives_shared_conditions_and_sorting_but_not_pages() {
        assert!(!render_shared(DeriveOptions::default()).contains("serde"));
        let rendered = render_shared(DeriveOptions {
            serde: true,
            ..DeriveOptions::default()
        });
        // Enums serialize their variants in snake_case, as JSON APIs expect.
        for item in [
            "pub enum Direction {",
            "pub enum Ordered<T> {",
            "pub enum Equality<T> {",
            "pub enum Text {",
        ] {
            assert!(
                rendered.contains(&format!(
                    "serde::Deserialize)]\n#[serde(rename_all = \"snake_case\")]\n{item}"
                )),
                "{item}:\n{rendered}"
            );
        }
        assert!(
            rendered.contains("serde::Deserialize)]\npub struct Sort<K> {"),
            "{rendered}"
        );
        for item in ["pub struct Page {", "pub struct Limit(i64);"] {
            assert!(
                rendered.contains(&format!("PartialEq, Eq)]\n{item}")),
                "{item}:\n{rendered}"
            );
        }
        assert_eq!(rendered.matches("serde::Serialize").count(), 5);
    }
}
