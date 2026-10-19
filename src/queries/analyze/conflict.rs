//! `ON CONFLICT` targets and `ON CONSTRAINT` names, checked against the
//! table's keys.

use sqlparser::ast::{Expr, Ident, ObjectName};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::keywords::Keyword;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Location, Token, TokenWithSpan, Tokenizer};

use crate::parser::postgres::{ident_name, object_leaf_name};
use crate::schema::Table;

/// A query's SQL with the `WHERE` of an `ON CONFLICT (cols) WHERE … DO`
/// target taken out, which the SQL parser cannot read, and that condition.
pub(super) struct ConflictPredicate {
    /// The SQL to analyze; the generated function keeps the SQL as written.
    pub sql: String,
    pub predicate: Option<Expr>,
}

/// Takes the index predicate out of an `ON CONFLICT (cols) WHERE … DO`
/// target. SQL that does not tokenize, or has no such target, comes back
/// as it is, for the parser to report on. A placeholder in the predicate is
/// an error: PostgreSQL infers the index when it plans the statement, before
/// any parameter is known.
pub(super) fn take_conflict_predicate(sql: &str) -> Result<ConflictPredicate, String> {
    let unchanged = || ConflictPredicate {
        sql: sql.to_owned(),
        predicate: None,
    };
    let dialect = PostgreSqlDialect {};
    let Ok(tokens) = Tokenizer::new(&dialect, sql).tokenize_with_location() else {
        return Ok(unchanged());
    };
    let Some((start, end)) = predicate_range(&tokens) else {
        return Ok(unchanged());
    };
    let predicate_tokens = &tokens[start + 1..end];
    if predicate_tokens
        .iter()
        .any(|token| matches!(token.token, Token::Placeholder(_)))
    {
        return Err(
            "a placeholder in the ON CONFLICT target's WHERE is not supported; PostgreSQL infers the index before parameters are bound"
                .to_owned(),
        );
    }
    let predicate = Parser::new(&dialect)
        .with_tokens_with_locations(predicate_tokens.to_vec())
        .parse_expr()
        .map_err(|error| format!("invalid SQL in the ON CONFLICT target's WHERE: {error}"))?;
    // Cut the original text rather than print the tokens again: a printed
    // token loses its escaping (`'it''s'` prints as `'it's'`).
    let (Some(cut_start), Some(cut_end)) = (
        byte_offset(sql, tokens[start].span.start),
        byte_offset(sql, tokens[end].span.start),
    ) else {
        return Ok(unchanged());
    };
    Ok(ConflictPredicate {
        sql: format!("{}{}", &sql[..cut_start], &sql[cut_end..]),
        predicate: Some(predicate),
    })
}

/// The byte offset in `sql` of a token location (1-based line and
/// character column).
fn byte_offset(sql: &str, location: Location) -> Option<usize> {
    let line = usize::try_from(location.line).ok()?.checked_sub(1)?;
    let column = usize::try_from(location.column).ok()?.checked_sub(1)?;
    let line_start: usize = sql.split_inclusive('\n').take(line).map(str::len).sum();
    let rest = &sql[line_start..];
    rest.char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(rest.len()))
        .nth(column)
        .map(|offset| line_start + offset)
}

/// The token range from the `WHERE` of an `ON CONFLICT (…) WHERE … DO`
/// target up to (not including) its `DO`.
fn predicate_range(tokens: &[TokenWithSpan]) -> Option<(usize, usize)> {
    let significant: Vec<usize> = (0..tokens.len())
        .filter(|&index| !matches!(tokens[index].token, Token::Whitespace(_) | Token::EOF))
        .collect();
    let keyword = |position: usize, keyword: Keyword| {
        significant.get(position).is_some_and(
            |&index| matches!(&tokens[index].token, Token::Word(word) if word.keyword == keyword),
        )
    };
    let on = (0..significant.len()).find(|&position| {
        keyword(position, Keyword::ON) && keyword(position + 1, Keyword::CONFLICT)
    })?;
    let mut position = on + 2;
    if tokens[*significant.get(position)?].token != Token::LParen {
        return None;
    }
    let mut depth = 0usize;
    loop {
        match tokens[*significant.get(position)?].token {
            Token::LParen => depth += 1,
            Token::RParen => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        position += 1;
    }
    position += 1;
    if !keyword(position, Keyword::WHERE) {
        return None;
    }
    let start = significant[position];
    let mut depth = 0usize;
    for &index in &significant[position + 1..] {
        match &tokens[index].token {
            Token::LParen => depth += 1,
            Token::RParen => depth = depth.saturating_sub(1),
            Token::Word(word) if depth == 0 && word.keyword == Keyword::DO => {
                return Some((start, index));
            }
            _ => {}
        }
    }
    None
}

/// Checks that an `ON CONFLICT (cols) [WHERE predicate]` target is one of
/// the table's unique keys (in any column order), so an upsert cannot fail
/// at runtime for want of a matching index. Without a predicate it must be
/// the primary key, a unique constraint, or a full unique index. With one,
/// it may also be a partial unique index whose condition is the same
/// predicate: PostgreSQL infers any index the predicate implies, and
/// rok-sqlc checks only an exact match.
pub(super) fn check_conflict_target(
    table: &Table,
    target: &[Ident],
    predicate: Option<&Expr>,
) -> Result<(), String> {
    let target: Vec<String> = target.iter().map(ident_name).collect();
    let same_columns = |key: &[String]| {
        key.len() == target.len() && target.iter().all(|column| key.contains(column))
    };
    let keys = table.unique_key_columns(None);
    if keys.iter().any(|key| same_columns(key)) {
        return Ok(());
    }
    let partial: Vec<&String> = table
        .unique_keys
        .iter()
        .filter(|key| !key.case_insensitive && same_columns(&key.columns))
        .filter_map(|key| key.predicate.as_ref())
        .collect();
    if let Some(predicate) = predicate {
        let predicate = condition(&predicate.to_string()).to_owned();
        if partial.iter().any(|key| condition(key) == predicate) {
            return Ok(());
        }
    }
    let target_text = target.join(", ");
    if let Some(condition) = partial.first() {
        let fix = match predicate {
            Some(predicate) => {
                format!("the target's `WHERE {predicate}` must be `WHERE {condition}`")
            }
            None => format!("add `WHERE {condition}` to the target"),
        };
        return Err(format!(
            "ON CONFLICT ({target_text}) matches only a partial unique index of `{}`, \
             whose condition is `{condition}`; {fix}",
            table.display_name()
        ));
    }
    let available = if keys.is_empty() {
        format!("`{}` has no unique keys", table.display_name())
    } else {
        let listed: Vec<String> = keys
            .iter()
            .map(|key| format!("({})", key.join(", ")))
            .collect();
        format!(
            "unique keys of `{}`: {}",
            table.display_name(),
            listed.join(", ")
        )
    };
    Err(format!(
        "ON CONFLICT ({target_text}) matches no unique key; {available}"
    ))
}

/// A condition without the parentheses around the whole of it.
fn condition(mut text: &str) -> &str {
    text = text.trim();
    while let Some(inner) = text
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
    {
        // `(a) AND (b)` is not wrapped: its first `(` closes early.
        let mut depth = 0i32;
        let balanced = inner.chars().all(|character| {
            match character {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            depth >= 0
        });
        if !balanced {
            break;
        }
        text = inner.trim();
    }
    text
}

/// Checks that `ON CONFLICT ON CONSTRAINT name` names one of the table's
/// primary key or unique constraints. PostgreSQL accepts only a constraint
/// there, never a unique index.
pub(super) fn check_conflict_constraint(table: &Table, name: &ObjectName) -> Result<(), String> {
    let wanted = object_leaf_name(name);
    let names: Vec<&str> = table
        .key_constraints
        .iter()
        .map(|constraint| constraint.name.as_str())
        .collect();
    if names.contains(&wanted.as_str()) {
        return Ok(());
    }
    let available = if names.is_empty() {
        "it has none".to_owned()
    } else {
        format!("its constraints are {}", names.join(", "))
    };
    Err(format!(
        "ON CONFLICT ON CONSTRAINT {wanted} names no primary key or unique constraint of `{}`; {available}",
        table.display_name()
    ))
}
