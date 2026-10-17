//! Schema-aware analysis of annotated queries: parameter types, result
//! columns, and the Rust shape of the result.
//!
//! Types are inferred only from contexts that determine them unambiguously:
//! a comparison, `IN`, `= ANY`, `BETWEEN`, or `LIKE` against a known column;
//! an `INSERT` value or `UPDATE … SET` target; `LIMIT`/`OFFSET`; and explicit
//! casts. Anything else must be declared with `-- param:` or `-- column:`;
//! the generator never guesses. A non-recursive, read-only CTE is analyzed
//! like a `SELECT` and then acts as a table for the statement that follows.

mod conflict;
mod params;
mod resolve;
mod results;
mod statements;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashSet};
use std::ops::Deref;
use std::rc::Rc;

use heck::ToSnakeCase;
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

use self::conflict::take_conflict_predicate;
use self::results::result_shape;
use super::annotations::{QueryDef, QueryFailure, ReturnMode};
use crate::parser::TableFailure;
use crate::parser::naming;
use crate::schema::{EnumType, Table};

/// A bound parameter, in placeholder order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// The placeholder number (`$1` is 1).
    pub index: usize,
    pub name: String,
    pub rust_type: String,
}

/// One column of a query's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputColumn {
    /// The name PostgreSQL reports for the column.
    pub sql_name: String,
    pub rust_name: String,
    pub rust_type: String,
}

/// The Rust type a query's rows decode into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultShape {
    /// `:exec` / `:execrows`: rows are not read.
    None,
    /// Every column of a generated resource, in order: its row struct is reused.
    Model { module: String, struct_name: String },
    /// A single column, decoded with `query_scalar`.
    Scalar(String),
    /// Several columns: a dedicated `{Name}Row` struct.
    Row(Vec<OutputColumn>),
}

/// A query ready for code generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedQuery {
    pub name: String,
    pub fn_name: String,
    pub mode: ReturnMode,
    /// The SQL as written, without its trailing `;`.
    pub sql: String,
    pub params: Vec<Param>,
    pub result: ResultShape,
}

/// Analyzes each query against the schema.
///
/// `tables` are all generatable tables, `enums` the generated enums (a cast
/// to a database enum type infers it), `generated_modules` the resource
/// modules emitted in this run (whose row structs may be reused), and
/// `unsupported` the tables the schema parser rejected, used to explain why a
/// query referencing one of them cannot be generated.
pub fn analyze_queries(
    defs: &[QueryDef],
    tables: &[Table],
    enums: &[EnumType],
    generated_modules: &HashSet<String>,
    unsupported: &[TableFailure],
) -> (Vec<AnalyzedQuery>, Vec<QueryFailure>) {
    let mut analyzed = Vec::new();
    let mut failures = Vec::new();
    let mut fn_names = HashSet::new();
    for def in defs {
        let context = Context {
            tables,
            enums,
            unsupported,
            generated_modules,
        };
        match analyze_one(def, &context) {
            Ok(query) if !fn_names.insert(query.fn_name.clone()) => failures.push(
                QueryFailure::new(
                    &def.name,
                    format!(
                        "function name `{}` is already used by another query",
                        query.fn_name
                    ),
                )
                .at_line(def.line),
            ),
            Ok(query) => analyzed.push(query),
            Err(reason) => failures.push(QueryFailure::new(&def.name, reason).at_line(def.line)),
        }
    }
    (analyzed, failures)
}

struct Context<'a> {
    tables: &'a [Table],
    /// The generated enums, for casts to an enum type.
    enums: &'a [EnumType],
    unsupported: &'a [TableFailure],
    generated_modules: &'a HashSet<String>,
}

fn analyze_one(def: &QueryDef, context: &Context<'_>) -> Result<AnalyzedQuery, String> {
    let fn_name = naming::rust_identifier(&def.name.to_snake_case())?;
    let conflict = take_conflict_predicate(&def.sql)?;
    let statements = Parser::parse_sql(&PostgreSqlDialect {}, &conflict.sql)
        .map_err(|error| format!("invalid SQL: {error}"))?;
    let statement = match statements.as_slice() {
        [statement] => statement,
        [] => return Err("the query has no SQL statement".to_owned()),
        many => {
            return Err(format!(
                "the query contains {} statements; generate one function per statement and run them in a caller-owned transaction (`&mut *tx`)",
                many.len()
            ));
        }
    };

    let mut analysis = Analysis::new(context);
    analysis.conflict_predicate = conflict.predicate;
    let outputs = analysis.statement(statement)?;
    let params = analysis.params(statement, def)?;
    let result = result_shape(def, outputs, context)?;

    Ok(AnalyzedQuery {
        name: def.name.clone(),
        fn_name,
        mode: def.mode,
        sql: def
            .sql
            .trim_end()
            .trim_end_matches(';')
            .trim_end()
            .to_owned(),
        params,
        result,
    })
}

/// A table visible to column references, under its alias or name.
struct ScopeEntry<'a> {
    qualifier: String,
    table: TableRef<'a>,
    /// An outer join can produce rows without a match on this side.
    outer_nullable: bool,
}

/// A result column with where it came from, for model reuse.
struct Output {
    sql_name: String,
    rust_type: Option<String>,
    /// `(resource module, column index)` for a plain column reference.
    origin: Option<(String, usize)>,
    nullable_origin: bool,
    /// Why the type could not be inferred, reported unless overridden.
    unknown_reason: Option<String>,
    /// Why the column has no name, reported unless a CTE column list names
    /// it; `sql_name` is empty then.
    name_error: Option<String>,
}

/// What a `FROM` item names: a schema table, or a CTE's result columns
/// shaped as a table.
#[derive(Clone)]
enum TableRef<'a> {
    Schema(&'a Table),
    Cte(Rc<Table>),
}

impl TableRef<'_> {
    /// The resource module whose row struct could be reused for this
    /// table's columns; a CTE has none.
    fn module(&self) -> Option<&str> {
        match self {
            TableRef::Schema(table) => Some(&table.module_name),
            TableRef::Cte(_) => None,
        }
    }
}

impl Deref for TableRef<'_> {
    type Target = Table;

    fn deref(&self) -> &Table {
        match self {
            TableRef::Schema(table) => table,
            TableRef::Cte(table) => table,
        }
    }
}

struct Slot {
    name: String,
    /// The parameter's type: a typed id when every column naming it has the
    /// same one, otherwise the plain type.
    base: String,
    /// The plain type under a typed id (`i64` for `CategoryId`); `base` itself
    /// for other types.
    raw: String,
    nullable: bool,
}

struct Analysis<'a, 'c> {
    context: &'c Context<'a>,
    scope: Vec<ScopeEntry<'a>>,
    /// CTEs visible by name, in definition order.
    ctes: Vec<Rc<Table>>,
    slots: BTreeMap<usize, Slot>,
    /// The `WHERE` of an `ON CONFLICT (cols) WHERE … DO` target, taken out of
    /// the SQL before parsing (see [`take_conflict_predicate`]).
    conflict_predicate: Option<sqlparser::ast::Expr>,
}

impl<'a, 'c> Analysis<'a, 'c> {
    fn new(context: &'c Context<'a>) -> Self {
        Self {
            context,
            scope: Vec::new(),
            ctes: Vec::new(),
            slots: BTreeMap::new(),
            conflict_predicate: None,
        }
    }
}
