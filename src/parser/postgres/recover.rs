//! Statements the SQL parser cannot read, recognised from their tokens when
//! their effect on the model is known: `ALTER TABLE` clauses that leave the
//! model unchanged or map onto clauses the parser reads, renames of
//! indexes, sequences, and views, and statements that cannot change the
//! model at all (functions, procedures, `DO` blocks).

use sqlparser::ast::ObjectName;
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::keywords::Keyword;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Token, TokenWithSpan};

/// Splits a file's tokens into statements at top-level semicolons. A
/// `BEGIN ATOMIC … END` function body, with any `CASE … END` inside it,
/// belongs to its statement.
pub(super) fn split_statements(tokens: &[TokenWithSpan]) -> Vec<&[TokenWithSpan]> {
    let mut statements = Vec::new();
    let mut start = 0;
    // `BEGIN ATOMIC` and `CASE` open a level that `END` closes.
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match &token.token {
            Token::SemiColon if depth == 0 => {
                statements.push(&tokens[start..index]);
                start = index + 1;
            }
            token
                if is_word(token, "BEGIN")
                    && depth == 0
                    && next_significant(&tokens[index + 1..])
                        .is_some_and(|next| is_word(&next.token, "ATOMIC")) =>
            {
                depth = 1;
            }
            token if depth > 0 && is_word(token, "CASE") => depth += 1,
            token if depth > 0 && is_word(token, "END") => depth -= 1,
            _ => {}
        }
    }
    statements.push(&tokens[start..]);
    statements
}

/// What an unreadable statement amounts to.
pub(super) enum Recovered {
    /// It cannot change the model: skip it.
    Skip,
    /// `ALTER {INDEX | SEQUENCE | VIEW | MATERIALIZED VIEW} [IF EXISTS] name
    /// RENAME TO new`.
    RenameRelation { name: ObjectName, new_name: String },
}

/// Recognises an unreadable statement other than `ALTER TABLE` whose effect
/// on the model is known: a function, procedure, or `DO` block, which
/// cannot change it, or an `ALTER` of an index, sequence, or view, which
/// changes it only by a rename. `None` for any other statement.
pub(super) fn recover_statement(
    dialect: &PostgreSqlDialect,
    chunk: &[TokenWithSpan],
) -> Option<Recovered> {
    let mut parser = Parser::new(dialect).with_tokens_with_locations(chunk.to_vec());
    if parser.parse_keyword(Keyword::DO) {
        return Some(Recovered::Skip);
    }
    if parser.parse_keyword(Keyword::CREATE) {
        let _ = parser.parse_keywords(&[Keyword::OR, Keyword::REPLACE]);
        return parser
            .parse_one_of_keywords(&[Keyword::FUNCTION, Keyword::PROCEDURE])
            .map(|_| Recovered::Skip);
    }
    if !parser.parse_keyword(Keyword::ALTER) {
        return None;
    }
    let relation = parser.parse_keyword(Keyword::INDEX)
        || parser.parse_keyword(Keyword::SEQUENCE)
        || parser.parse_keyword(Keyword::VIEW)
        || parser.parse_keywords(&[Keyword::MATERIALIZED, Keyword::VIEW]);
    if !relation {
        return None;
    }
    let _ = parser.parse_keywords(&[Keyword::IF, Keyword::EXISTS]);
    let rename = parser.parse_object_name(false).ok().and_then(|name| {
        parser
            .parse_keywords(&[Keyword::RENAME, Keyword::TO])
            .then(|| parser.parse_identifier().ok())
            .flatten()
            .filter(|_| parser.peek_token().token == Token::EOF)
            .map(|new_name| Recovered::RenameRelation {
                name,
                new_name: super::names::ident_name(&new_name),
            })
    });
    // Any other change to an index, sequence, or view leaves the tables as
    // they are.
    Some(rename.unwrap_or(Recovered::Skip))
}

/// An unreadable `ALTER TABLE`, rewritten so the SQL parser reads it.
pub(super) enum AlterRewrite {
    /// Every clause leaves the model unchanged.
    Skip,
    /// The statement with each recognised clause dropped or replaced by one
    /// the parser reads.
    Tokens(Vec<TokenWithSpan>),
}

/// Rewrites an `ALTER TABLE` statement the parser cannot read, clause by
/// clause:
///
/// - `ALTER COLUMN c` with `SET STATISTICS`, `SET STORAGE`, `SET
///   COMPRESSION`, `SET (…)`, `RESET (…)`, `SET GENERATED …`, `RESTART`, or
///   another identity sequence option leaves the model unchanged: the
///   clause is dropped;
/// - `ALTER COLUMN c DROP IDENTITY` or `DROP EXPRESSION` (with or without
///   `IF EXISTS`) leaves the column with no default and no longer
///   generated, which is what `DROP DEFAULT` does to the model: the clause
///   becomes one;
/// - `ALTER COLUMN c [SET DATA] TYPE t COLLATE x …` loses `COLLATE x`;
/// - `RESET (…)`, `CLUSTER ON`, `SET WITHOUT CLUSTER`, `SET TABLESPACE`,
///   `SET LOGGED`, `SET UNLOGGED`, `[NO] FORCE ROW LEVEL SECURITY`, and
///   `ALTER CONSTRAINT` leave the model unchanged: the clause is dropped.
///
/// `None` when the statement is no `ALTER TABLE`, a clause is none of
/// these and the parser cannot read it, or the rewritten statement still
/// does not parse.
pub(super) fn rewrite_alter_table(
    dialect: &PostgreSqlDialect,
    chunk: &[TokenWithSpan],
) -> Option<AlterRewrite> {
    let mut parser = Parser::new(dialect).with_tokens_with_locations(chunk.to_vec());
    if !parser.parse_keywords(&[Keyword::ALTER, Keyword::TABLE]) {
        return None;
    }
    let _ = parser.parse_keywords(&[Keyword::IF, Keyword::EXISTS]);
    let _ = parser.parse_keyword(Keyword::ONLY);
    parser.parse_object_name(false).ok()?;
    let header = &chunk[..parser.index()];

    let mut kept: Vec<Vec<TokenWithSpan>> = Vec::new();
    for clause in split_clauses(&chunk[header.len()..]) {
        let mut statement = header.to_vec();
        statement.extend_from_slice(clause);
        if parses(dialect, statement) {
            kept.push(clause.to_vec());
            continue;
        }
        match rewrite_clause(clause)? {
            ClauseRewrite::Drop => {}
            ClauseRewrite::Replace(tokens) => kept.push(tokens),
        }
    }
    if kept.is_empty() {
        return Some(AlterRewrite::Skip);
    }
    let mut statement = header.to_vec();
    for (index, clause) in kept.into_iter().enumerate() {
        if index > 0 {
            statement.push(TokenWithSpan::wrap(Token::Comma));
        }
        statement.extend(clause);
    }
    parses(dialect, statement.clone()).then_some(AlterRewrite::Tokens(statement))
}

enum ClauseRewrite {
    Drop,
    Replace(Vec<TokenWithSpan>),
}

/// Recognises one unreadable `ALTER TABLE` clause (see
/// [`rewrite_alter_table`]).
fn rewrite_clause(clause: &[TokenWithSpan]) -> Option<ClauseRewrite> {
    let words: Vec<&TokenWithSpan> = clause
        .iter()
        .filter(|token| !matches!(token.token, Token::Whitespace(_) | Token::EOF))
        .collect();
    let at = |index: usize, word: &str| words.get(index).is_some_and(|t| is_word(&t.token, word));
    let open_paren_at = |index: usize| words.get(index).is_some_and(|t| t.token == Token::LParen);

    // Table-level clauses that leave the model unchanged.
    let table_level = (at(0, "RESET") && open_paren_at(1))
        || (at(0, "CLUSTER") && at(1, "ON"))
        || (at(0, "SET") && at(1, "WITHOUT") && at(2, "CLUSTER"))
        || (at(0, "SET") && (at(1, "TABLESPACE") || at(1, "LOGGED") || at(1, "UNLOGGED")))
        || (at(0, "FORCE") && at(1, "ROW") && at(2, "LEVEL") && at(3, "SECURITY"))
        || (at(0, "NO") && at(1, "FORCE") && at(2, "ROW") && at(3, "LEVEL"))
        || (at(0, "ALTER") && at(1, "CONSTRAINT"));
    if table_level {
        return Some(ClauseRewrite::Drop);
    }

    if !at(0, "ALTER") {
        return None;
    }
    // `ALTER [COLUMN] name`: the operation starts after the name.
    let name = if at(1, "COLUMN") { 2 } else { 1 };
    words
        .get(name)
        .filter(|t| matches!(t.token, Token::Word(_)))?;
    let op = name + 1;

    let sequence_option = |index: usize| {
        [
            "INCREMENT",
            "MINVALUE",
            "MAXVALUE",
            "START",
            "CACHE",
            "CYCLE",
            "NO",
            "OWNED",
            "SEQUENCE",
        ]
        .iter()
        .any(|word| at(index, word))
    };
    let unchanged = (at(op, "SET")
        && (at(op + 1, "STATISTICS")
            || at(op + 1, "STORAGE")
            || at(op + 1, "COMPRESSION")
            || at(op + 1, "GENERATED")
            || open_paren_at(op + 1)
            || sequence_option(op + 1)))
        || (at(op, "RESET") && open_paren_at(op + 1))
        || at(op, "RESTART");
    if unchanged {
        return Some(ClauseRewrite::Drop);
    }

    if at(op, "DROP") && (at(op + 1, "IDENTITY") || at(op + 1, "EXPRESSION")) {
        let rest = words.len() - (op + 2);
        let if_exists = rest == 2 && at(op + 2, "IF") && at(op + 3, "EXISTS");
        if rest != 0 && !if_exists {
            return None;
        }
        let mut tokens: Vec<TokenWithSpan> =
            words[..op].iter().map(|token| (*token).clone()).collect();
        let span = words[op].span;
        for keyword in ["DROP", "DEFAULT"] {
            tokens.push(TokenWithSpan::new(Token::make_keyword(keyword), span));
        }
        return Some(ClauseRewrite::Replace(spaced(tokens)));
    }

    // `[SET DATA] TYPE t COLLATE x [USING …]`: drop `COLLATE x`.
    let is_type = at(op, "TYPE") || (at(op, "SET") && at(op + 1, "DATA") && at(op + 2, "TYPE"));
    let collate = clause
        .iter()
        .position(|token| is_word(&token.token, "COLLATE"));
    if let (true, Some(collate)) = (is_type, collate) {
        let after = collate_name_end(clause, collate + 1)?;
        let mut tokens = clause[..collate].to_vec();
        tokens.extend_from_slice(&clause[after..]);
        return Some(ClauseRewrite::Replace(tokens));
    }
    None
}

/// The index just past the (possibly qualified) collation name that starts
/// at or after `start`.
fn collate_name_end(clause: &[TokenWithSpan], start: usize) -> Option<usize> {
    let mut index = start;
    let mut expect_name = true;
    while let Some(token) = clause.get(index) {
        match &token.token {
            Token::Whitespace(_) => {}
            Token::Word(_) if expect_name => expect_name = false,
            Token::Period if !expect_name => expect_name = true,
            _ => break,
        }
        index += 1;
    }
    (!expect_name).then_some(index)
}

/// Puts a space between tokens that came from different places, so a
/// rebuilt clause renders and parses as words.
fn spaced(tokens: Vec<TokenWithSpan>) -> Vec<TokenWithSpan> {
    let mut spaced = Vec::with_capacity(tokens.len() * 2);
    for token in tokens {
        let needs_space = !matches!(token.token, Token::Whitespace(_))
            && spaced
                .last()
                .is_some_and(|last: &TokenWithSpan| !matches!(last.token, Token::Whitespace(_)));
        if needs_space {
            spaced.push(TokenWithSpan::new(
                Token::Whitespace(sqlparser::tokenizer::Whitespace::Space),
                token.span,
            ));
        }
        spaced.push(token);
    }
    spaced
}

/// Splits an `ALTER TABLE`'s clauses at top-level commas.
fn split_clauses(tokens: &[TokenWithSpan]) -> Vec<&[TokenWithSpan]> {
    let mut clauses = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (index, token) in tokens.iter().enumerate() {
        match token.token {
            Token::LParen => depth += 1,
            Token::RParen => depth = depth.saturating_sub(1),
            Token::Comma if depth == 0 => {
                clauses.push(&tokens[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    clauses.push(&tokens[start..]);
    clauses
}

fn parses(dialect: &PostgreSqlDialect, tokens: Vec<TokenWithSpan>) -> bool {
    Parser::new(dialect)
        .with_tokens_with_locations(tokens)
        .parse_statements()
        .is_ok()
}

fn next_significant(tokens: &[TokenWithSpan]) -> Option<&TokenWithSpan> {
    tokens
        .iter()
        .find(|token| !matches!(token.token, Token::Whitespace(_)))
}

/// Whether `token` is the unquoted word `word`, in any case.
fn is_word(token: &Token, word: &str) -> bool {
    matches!(token, Token::Word(w) if w.quote_style.is_none() && w.value.eq_ignore_ascii_case(word))
}
