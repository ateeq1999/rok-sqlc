//! SQL text rendering shared by every generated statement: identifier
//! quoting, table references, column lists, and embedding SQL in Rust
//! string literals.
//!
//! Only schema metadata and fixed generator-owned fragments ever reach these
//! functions; runtime values are always bound with `.bind`/`push_bind`.

use crate::schema::{Column, Table};

/// Renders a SQL identifier, quoting it only when PostgreSQL requires it:
/// when it is not a plain lowercase identifier, or when it is a keyword that
/// cannot be used bare as a column or table name.
pub fn ident(name: &str) -> String {
    if is_plain_identifier(name) && !is_restricted_keyword(name) {
        name.to_owned()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

/// Renders the table reference, keeping the schema qualifier when present.
pub fn table_ref(table: &Table) -> String {
    match &table.schema {
        Some(schema) => format!("{}.{}", ident(schema), ident(&table.sql_name)),
        None => ident(&table.sql_name),
    }
}

/// Every column of the table, in declaration order.
pub fn column_names(table: &Table) -> Vec<String> {
    table
        .columns
        .iter()
        .map(|column| ident(&column.sql_name))
        .collect()
}

/// A keyword followed by every column of the table, one per line:
/// `["SELECT", "    a,", "    b"]`.
pub fn column_clause(keyword: &str, table: &Table) -> Vec<String> {
    let names = column_names(table);
    let mut lines = vec![keyword.to_owned()];
    for (index, name) in names.iter().enumerate() {
        let separator = if index + 1 < names.len() { "," } else { "" };
        lines.push(format!("    {name}{separator}"));
    }
    lines
}

/// `a = $1 AND b = $2` over the given key columns, numbering from `first`.
pub fn key_predicate(keys: &[&Column], first: usize) -> String {
    keys.iter()
        .enumerate()
        .map(|(index, column)| format!("{} = ${}", ident(&column.sql_name), first + index))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// A column reference, qualified with `alias` when there is one: `a.col`.
fn column_ref(alias: Option<&str>, column: &str) -> String {
    match alias {
        Some(alias) => format!("{alias}.{}", ident(column)),
        None => ident(column),
    }
}

/// `[alias.]<soft-delete column> IS NULL`, the condition selecting live
/// rows, or `None` when the table has no soft-delete column.
pub fn live_condition(table: &Table, alias: Option<&str>) -> Option<String> {
    table
        .soft_delete_column
        .as_deref()
        .map(|column| format!("{} IS NULL", column_ref(alias, column)))
}

/// `[alias.]<tenant column> = $1` when the table has a tenant column: static
/// SQL always binds the tenant parameter first.
pub fn tenant_condition(table: &Table, alias: Option<&str>) -> Option<String> {
    table
        .tenant()
        .map(|tenant| format!("{} = $1", column_ref(alias, &tenant.sql_name)))
}

/// The placeholder of the first value bound after the tenant parameter:
/// `$2` on a table with a tenant column, `$1` otherwise.
pub fn first_placeholder(table: &Table) -> usize {
    1 + usize::from(table.tenant().is_some())
}

/// The tenant condition (when the table has a tenant column) followed by
/// `key_predicate` over `keys`, numbered after the tenant's placeholder.
pub fn scoped_key_predicate(table: &Table, keys: &[&Column]) -> String {
    let tenant = tenant_condition(table, None);
    let first = first_placeholder(table);
    tenant
        .into_iter()
        .chain((!keys.is_empty()).then(|| key_predicate(keys, first)))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// `scoped_key_predicate`, followed by `AND <soft-delete column> IS NULL`
/// when the table has one, so only a live row matches.
pub fn live_key_predicate(table: &Table, keys: &[&Column]) -> String {
    let predicate = scoped_key_predicate(table, keys);
    match live_condition(table, None) {
        Some(live) => format!("{predicate} AND {live}"),
        None => predicate,
    }
}

/// The condition selecting every row a whole-table read sees: the tenant's
/// rows (`$1`) that are live, or `None` when every row is visible.
pub fn scope_condition(table: &Table) -> Option<String> {
    let conditions: Vec<String> = tenant_condition(table, None)
        .into_iter()
        .chain(live_condition(table, None))
        .collect();
    (!conditions.is_empty()).then(|| conditions.join(" AND "))
}

/// `a, b` over the given key columns, for `ORDER BY`.
pub fn key_order(keys: &[&Column]) -> String {
    keys.iter()
        .map(|column| ident(&column.sql_name))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Renders `text` as a Rust string literal: a plain literal when no escaping
/// is needed, otherwise a raw literal with enough `#`s to be unambiguous.
pub fn str_literal(text: &str) -> String {
    if text.contains(['"', '\\']) {
        let hashes = raw_hashes(text);
        format!("r{hashes}\"{text}\"{hashes}")
    } else {
        format!("\"{text}\"")
    }
}

/// The `#` delimiter a raw string literal around `text` needs: one more than
/// the longest `"#…` run inside it, and at least one.
pub fn raw_hashes(text: &str) -> String {
    let mut depth = 1;
    while text.contains(&format!("\"{}", "#".repeat(depth))) {
        depth += 1;
    }
    "#".repeat(depth)
}

/// Renders SQL lines as a multi-line raw string literal whose content and
/// delimiters sit at `indent` columns, matching rustfmt's layout for a
/// vertical call argument. No trailing comma is emitted.
pub fn raw_block(lines: &[String], indent: usize) -> String {
    let pad = " ".repeat(indent);
    let hashes = raw_hashes(&lines.join("\n"));
    let mut block = format!("{pad}r{hashes}\"\n");
    for line in lines {
        block.push_str(&format!("{pad}{line}\n"));
    }
    block.push_str(&format!("{pad}\"{hashes}"));
    block
}

fn is_plain_identifier(name: &str) -> bool {
    let mut characters = name.chars();
    matches!(characters.next(), Some('a'..='z' | '_'))
        && characters.all(|character| matches!(character, 'a'..='z' | '0'..='9' | '_'))
}

/// PostgreSQL keywords in the reserved, type/function-name, and column-name
/// categories (`pg_get_keywords()` catcodes R, T, and C). Quoting them
/// everywhere is always valid, and keeps the rule independent of the
/// position the identifier appears in.
fn is_restricted_keyword(name: &str) -> bool {
    RESTRICTED_KEYWORDS.binary_search(&name).is_ok()
}

const RESTRICTED_KEYWORDS: &[&str] = &[
    "all",
    "analyse",
    "analyze",
    "and",
    "any",
    "array",
    "as",
    "asc",
    "asymmetric",
    "authorization",
    "between",
    "bigint",
    "binary",
    "bit",
    "boolean",
    "both",
    "case",
    "cast",
    "char",
    "character",
    "check",
    "coalesce",
    "collate",
    "collation",
    "column",
    "concurrently",
    "constraint",
    "create",
    "cross",
    "current_catalog",
    "current_date",
    "current_role",
    "current_schema",
    "current_time",
    "current_timestamp",
    "current_user",
    "dec",
    "decimal",
    "default",
    "deferrable",
    "desc",
    "distinct",
    "do",
    "else",
    "end",
    "except",
    "exists",
    "extract",
    "false",
    "fetch",
    "float",
    "for",
    "foreign",
    "freeze",
    "from",
    "full",
    "grant",
    "greatest",
    "group",
    "grouping",
    "having",
    "ilike",
    "in",
    "initially",
    "inner",
    "inout",
    "int",
    "integer",
    "intersect",
    "interval",
    "into",
    "is",
    "isnull",
    "join",
    "json",
    "json_array",
    "json_arrayagg",
    "json_exists",
    "json_object",
    "json_objectagg",
    "json_query",
    "json_scalar",
    "json_serialize",
    "json_table",
    "json_value",
    "lateral",
    "leading",
    "least",
    "left",
    "like",
    "limit",
    "localtime",
    "localtimestamp",
    "merge_action",
    "national",
    "natural",
    "nchar",
    "none",
    "normalize",
    "not",
    "notnull",
    "null",
    "nullif",
    "numeric",
    "offset",
    "on",
    "only",
    "or",
    "order",
    "out",
    "outer",
    "overlaps",
    "overlay",
    "placing",
    "position",
    "precision",
    "primary",
    "real",
    "references",
    "returning",
    "right",
    "row",
    "select",
    "session_user",
    "setof",
    "similar",
    "smallint",
    "some",
    "substring",
    "symmetric",
    "system_user",
    "table",
    "tablesample",
    "then",
    "time",
    "timestamp",
    "to",
    "trailing",
    "treat",
    "trim",
    "true",
    "union",
    "unique",
    "user",
    "using",
    "values",
    "varchar",
    "variadic",
    "verbose",
    "when",
    "where",
    "window",
    "with",
    "xmlattributes",
    "xmlconcat",
    "xmlelement",
    "xmlexists",
    "xmlforest",
    "xmlnamespaces",
    "xmlparse",
    "xmlpi",
    "xmlroot",
    "xmlserialize",
    "xmltable",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_list_is_sorted_for_binary_search() {
        assert!(RESTRICTED_KEYWORDS.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn quotes_only_when_required() {
        assert_eq!(ident("price_cents"), "price_cents");
        assert_eq!(ident("_hidden"), "_hidden");
        assert_eq!(ident("type"), "type");
        assert_eq!(ident("name"), "name");
        assert_eq!(ident("select"), "\"select\"");
        assert_eq!(ident("user"), "\"user\"");
        assert_eq!(ident("time"), "\"time\"");
        assert_eq!(ident("Email"), "\"Email\"");
        assert_eq!(ident("Mixed Case"), "\"Mixed Case\"");
        assert_eq!(ident("1st"), "\"1st\"");
        assert_eq!(ident("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn literals_escape_only_when_needed() {
        assert_eq!(str_literal("price_cents"), "\"price_cents\"");
        assert_eq!(str_literal("\"select\""), "r#\"\"select\"\"#");
        assert_eq!(str_literal("\"#odd\""), "r##\"\"#odd\"\"##");
    }

    #[test]
    fn raw_block_picks_safe_delimiter() {
        let block = raw_block(&["SELECT \"#x\"".to_owned()], 4);
        assert_eq!(block, "    r##\"\n    SELECT \"#x\"\n    \"##");
    }
}
