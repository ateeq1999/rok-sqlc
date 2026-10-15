//! Splitting a query file into annotated queries.
//!
//! ```sql
//! -- name: ListProductsByCategory :many
//! -- param: $2 max_stock i32
//! SELECT id, sku FROM products WHERE category_id = $1 AND stock_on_hand <= $2;
//! ```
//!
//! `-- name:` starts a query; `-- param:` and `-- column:` lines that follow
//! it override inferred parameter and result-column types. Other full-line
//! comments are dropped; everything else up to the next `-- name:` is the
//! query's SQL.

use std::collections::HashSet;
use std::fmt;

/// How a query's result is fetched and returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnMode {
    /// Exactly one row; no row is `sqlx::Error::RowNotFound`.
    One,
    /// Zero or one row.
    MaybeOne,
    /// Any number of rows.
    Many,
    /// No result.
    Exec,
    /// No result besides the number of affected rows.
    ExecRows,
}

impl ReturnMode {
    const ALL: [ReturnMode; 5] = [
        ReturnMode::One,
        ReturnMode::MaybeOne,
        ReturnMode::Many,
        ReturnMode::Exec,
        ReturnMode::ExecRows,
    ];

    /// The annotation keyword, without the leading `:`.
    pub fn keyword(self) -> &'static str {
        match self {
            ReturnMode::One => "one",
            ReturnMode::MaybeOne => "maybe_one",
            ReturnMode::Many => "many",
            ReturnMode::Exec => "exec",
            ReturnMode::ExecRows => "execrows",
        }
    }

    /// Whether the mode returns result rows.
    pub fn returns_rows(self) -> bool {
        matches!(
            self,
            ReturnMode::One | ReturnMode::MaybeOne | ReturnMode::Many
        )
    }

    fn parse(keyword: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.keyword() == keyword)
    }
}

/// An explicit `-- param: $N name Type` override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamOverride {
    pub index: usize,
    pub name: String,
    pub rust_type: String,
}

/// An explicit `-- column: name Type` override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnOverride {
    pub name: String,
    pub rust_type: String,
}

/// One annotated query, before schema analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryDef {
    pub name: String,
    pub mode: ReturnMode,
    /// The SQL as written, without directive lines.
    pub sql: String,
    /// 1-based line of the `-- name:` annotation.
    pub line: usize,
    pub params: Vec<ParamOverride>,
    pub columns: Vec<ColumnOverride>,
}

/// A query that cannot be generated, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryFailure {
    /// The query name, or `line N` when no valid name was annotated.
    pub query: String,
    pub reason: String,
    /// 1-based line of the query's `-- name:` annotation, when known.
    pub line: Option<usize>,
}

impl fmt::Display for QueryFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "query `{}`: {}", self.query, self.reason)
    }
}

impl QueryFailure {
    pub fn new(query: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            reason: reason.into(),
            line: None,
        }
    }

    /// The same failure, located at the query's annotation line.
    pub fn at_line(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }
}

struct Pending {
    def: Option<QueryDef>,
    errors: Vec<String>,
    label: String,
    /// Line of the `-- name:` annotation.
    line: usize,
}

/// Splits `text` into annotated queries. Malformed annotations and queries
/// are reported as failures; well-formed queries are still returned.
pub fn parse_query_file(text: &str) -> (Vec<QueryDef>, Vec<QueryFailure>) {
    let mut queries = Vec::new();
    let mut failures = Vec::new();
    let mut pending: Option<Pending> = None;
    let mut reported_stray_sql = false;

    for (index, raw_line) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();

        if let Some(comment) = line.strip_prefix("--") {
            let comment = comment.trim();
            if let Some(header) = comment.strip_prefix("name:") {
                finish(pending.take(), &mut queries, &mut failures);
                pending = Some(start(header, line_number));
                continue;
            }
            for (prefix, parse) in [
                (
                    "param:",
                    parse_param as fn(&str, &mut QueryDef) -> Result<(), String>,
                ),
                ("column:", parse_column),
            ] {
                if let Some(directive) = comment.strip_prefix(prefix) {
                    match &mut pending {
                        Some(Pending {
                            def: Some(def),
                            errors,
                            ..
                        }) => {
                            if let Err(error) = parse(directive.trim(), def) {
                                errors.push(format!("line {line_number}: {error}"));
                            }
                        }
                        Some(_) => {}
                        None => failures.push(
                            QueryFailure::new(
                                format!("line {line_number}"),
                                format!("`-- {prefix}` appears before any `-- name:` annotation"),
                            )
                            .at_line(line_number),
                        ),
                    }
                }
            }
            continue;
        }

        match &mut pending {
            Some(Pending { def: Some(def), .. }) => {
                def.sql.push_str(raw_line.trim_end());
                def.sql.push('\n');
            }
            Some(_) => {}
            None if !line.is_empty() && !reported_stray_sql => {
                reported_stray_sql = true;
                failures.push(
                    QueryFailure::new(
                        format!("line {line_number}"),
                        "SQL appears before the first `-- name:` annotation",
                    )
                    .at_line(line_number),
                );
            }
            None => {}
        }
    }
    finish(pending, &mut queries, &mut failures);

    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for query in queries {
        if seen.insert(query.name.clone()) {
            unique.push(query);
        } else {
            failures.push(
                QueryFailure::new(
                    query.name.clone(),
                    format!(
                        "line {}: the name is already used by another query",
                        query.line
                    ),
                )
                .at_line(query.line),
            );
        }
    }
    (unique, failures)
}

fn start(header: &str, line: usize) -> Pending {
    let parts: Vec<&str> = header.split_whitespace().collect();
    let label = parts
        .first()
        .map(|name| (*name).to_owned())
        .unwrap_or_else(|| format!("line {line}"));
    let fail = |error: String| Pending {
        def: None,
        errors: vec![format!("line {line}: {error}")],
        label: label.clone(),
        line,
    };

    let [name, mode] = parts.as_slice() else {
        return fail(format!(
            "expected `-- name: <Name> :<mode>`, found `-- name:{header}`"
        ));
    };
    if !is_upper_camel_identifier(name) {
        return fail(format!(
            "query name `{name}` must be an UpperCamelCase identifier such as `GetUserById`"
        ));
    }
    let Some(mode) = mode.strip_prefix(':').and_then(ReturnMode::parse) else {
        let modes: Vec<String> = ReturnMode::ALL
            .iter()
            .map(|mode| format!(":{}", mode.keyword()))
            .collect();
        return fail(format!(
            "unknown return mode `{mode}`; expected one of {}",
            modes.join(", ")
        ));
    };

    Pending {
        def: Some(QueryDef {
            name: (*name).to_owned(),
            mode,
            sql: String::new(),
            line,
            params: Vec::new(),
            columns: Vec::new(),
        }),
        errors: Vec::new(),
        label,
        line,
    }
}

fn finish(pending: Option<Pending>, queries: &mut Vec<QueryDef>, failures: &mut Vec<QueryFailure>) {
    let Some(mut pending) = pending else {
        return;
    };
    if let Some(def) = &mut pending.def {
        def.sql = def.sql.trim().to_owned();
        if def.sql.is_empty() {
            pending
                .errors
                .push(format!("line {}: the query has no SQL", def.line));
        }
    }
    if pending.errors.is_empty()
        && let Some(def) = pending.def
    {
        queries.push(def);
    } else {
        failures.push(
            QueryFailure::new(pending.label, pending.errors.join("; ")).at_line(pending.line),
        );
    }
}

fn parse_param(directive: &str, def: &mut QueryDef) -> Result<(), String> {
    let usage = "expected `-- param: $<N> <name> <RustType>`";
    let mut parts = directive.splitn(3, char::is_whitespace);
    let (Some(placeholder), Some(name), Some(rust_type)) =
        (parts.next(), parts.next(), parts.next())
    else {
        return Err(usage.to_owned());
    };
    let index = placeholder
        .strip_prefix('$')
        .and_then(|digits| digits.parse::<usize>().ok())
        .filter(|index| *index > 0)
        .ok_or_else(|| format!("{usage}; `{placeholder}` is not a placeholder"))?;
    if !is_snake_identifier(name) {
        return Err(format!(
            "parameter name `{name}` must be a snake_case identifier"
        ));
    }
    if def.params.iter().any(|param| param.index == index) {
        return Err(format!("`${index}` is overridden more than once"));
    }
    def.params.push(ParamOverride {
        index,
        name: name.to_owned(),
        rust_type: rust_type.trim().to_owned(),
    });
    Ok(())
}

fn parse_column(directive: &str, def: &mut QueryDef) -> Result<(), String> {
    let mut parts = directive.splitn(2, char::is_whitespace);
    let (Some(name), Some(rust_type)) = (parts.next(), parts.next()) else {
        return Err("expected `-- column: <name> <RustType>`".to_owned());
    };
    if def.columns.iter().any(|column| column.name == name) {
        return Err(format!("column `{name}` is overridden more than once"));
    }
    def.columns.push(ColumnOverride {
        name: name.to_owned(),
        rust_type: rust_type.trim().to_owned(),
    });
    Ok(())
}

fn is_upper_camel_identifier(name: &str) -> bool {
    name.starts_with(|character: char| character.is_ascii_uppercase())
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

fn is_snake_identifier(name: &str) -> bool {
    name.starts_with(|character: char| character.is_ascii_lowercase() || character == '_')
        && name
            .chars()
            .all(|character| matches!(character, 'a'..='z' | '0'..='9' | '_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> (Vec<QueryDef>, Vec<QueryFailure>) {
        parse_query_file(text)
    }

    #[test]
    fn splits_annotated_queries_and_drops_comments() {
        let (queries, failures) = parse(
            "-- leading comment\n\n-- name: GetUser :maybe_one\n-- explains the query\nSELECT id\nFROM users\nWHERE id = $1;\n\n-- name: ListUsers :many\nSELECT id FROM users;\n",
        );
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(queries.len(), 2);
        assert_eq!(queries[0].name, "GetUser");
        assert_eq!(queries[0].mode, ReturnMode::MaybeOne);
        assert_eq!(queries[0].sql, "SELECT id\nFROM users\nWHERE id = $1;");
        assert_eq!(queries[0].line, 3);
        assert_eq!(queries[1].mode, ReturnMode::Many);
    }

    #[test]
    fn parses_every_return_mode() {
        for (keyword, mode) in [
            ("one", ReturnMode::One),
            ("maybe_one", ReturnMode::MaybeOne),
            ("many", ReturnMode::Many),
            ("exec", ReturnMode::Exec),
            ("execrows", ReturnMode::ExecRows),
        ] {
            let (queries, _) = parse(&format!("-- name: Q :{keyword}\nSELECT 1;"));
            assert_eq!(queries[0].mode, mode);
        }
    }

    #[test]
    fn parses_overrides() {
        let (queries, failures) = parse(
            "-- name: Search :many\n-- param: $2 max_stock i32\n-- column: total Option<i64>\nSELECT 1;",
        );
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(
            queries[0].params,
            [ParamOverride {
                index: 2,
                name: "max_stock".to_owned(),
                rust_type: "i32".to_owned()
            }]
        );
        assert_eq!(
            queries[0].columns,
            [ColumnOverride {
                name: "total".to_owned(),
                rust_type: "Option<i64>".to_owned()
            }]
        );
    }

    #[test]
    fn reports_malformed_annotations_without_losing_other_queries() {
        let (queries, failures) = parse(
            "-- name: bad_name :one\nSELECT 1;\n-- name: NoMode\nSELECT 1;\n-- name: Weird :first\nSELECT 1;\n-- name: Good :exec\nDELETE FROM t;\n",
        );
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].name, "Good");
        assert_eq!(failures.len(), 3);
        assert!(failures[0].reason.contains("UpperCamelCase"));
        assert!(
            failures[1]
                .reason
                .contains("expected `-- name: <Name> :<mode>`")
        );
        assert!(failures[2].reason.contains("unknown return mode `:first`"));
        assert!(failures[2].reason.contains(":maybe_one"));
    }

    #[test]
    fn reports_bad_overrides() {
        let (queries, failures) =
            parse("-- name: Q :one\n-- param: 2 x i32\n-- param: $1 Bad i32\nSELECT $1;");
        assert!(queries.is_empty());
        assert_eq!(failures.len(), 1);
        assert!(failures[0].reason.contains("line 2"));
        assert!(failures[0].reason.contains("not a placeholder"));
        assert!(failures[0].reason.contains("snake_case"));
    }

    #[test]
    fn reports_stray_sql_empty_queries_and_duplicates() {
        let (queries, failures) = parse(
            "SELECT 1;\n-- name: Empty :exec\n\n-- name: Twice :exec\nSELECT 1;\n-- name: Twice :exec\nSELECT 2;\n",
        );
        assert_eq!(queries.len(), 1);
        let reasons: Vec<&str> = failures
            .iter()
            .map(|failure| failure.reason.as_str())
            .collect();
        assert!(reasons[0].contains("before the first `-- name:`"));
        assert!(reasons[1].contains("has no SQL"));
        assert!(reasons[2].contains("already used"));
    }
}
