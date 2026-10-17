//! Schema-aware analysis of annotated queries: parameter types, result
//! columns, and the Rust shape of the result.
//!
//! Types are inferred only from contexts that determine them unambiguously:
//! a comparison, `IN`, `= ANY`, `BETWEEN`, or `LIKE` against a known column;
//! an `INSERT` value or `UPDATE … SET` target; `LIMIT`/`OFFSET`; and explicit
//! casts. Anything else must be declared with `-- param:` or `-- column:`;
//! the generator never guesses. A non-recursive, read-only CTE is analyzed
//! like a `SELECT` and then acts as a table for the statement that follows.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ops::{ControlFlow, Deref};
use std::rc::Rc;

use heck::ToSnakeCase;
use sqlparser::ast::{
    AssignmentTarget, BinaryOperator, ConflictTarget, Cte, Expr, FromTable, FunctionArg,
    FunctionArgExpr, FunctionArguments, Ident, JoinConstraint, JoinOperator, LimitClause,
    ObjectName, ObjectNamePart, OnConflictAction, OnInsert, Query, Select, SelectItem, SetExpr,
    Statement, TableFactor, TableObject, TableWithJoins, Value, visit_expressions,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

use super::annotations::{QueryDef, QueryFailure, ReturnMode};
use crate::parser::TableFailure;
use crate::parser::naming;
use crate::parser::postgres::{ident_name, map_type, object_leaf_name};
use crate::schema::{Column, EnumType, Shape, Table};

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

/// Checks that an `ON CONFLICT (cols)` target is one of the table's unique
/// keys (in any column order), so an upsert cannot fail at runtime for want
/// of a matching constraint. A partial index never matches: PostgreSQL infers
/// it only from a target with the index's `WHERE` condition, which query
/// files cannot express.
fn check_conflict_target(table: &Table, target: &[Ident]) -> Result<(), String> {
    let target: Vec<String> = target.iter().map(ident_name).collect();
    let keys = table.unique_key_columns(None);
    let matches = |key: &Vec<String>| {
        key.len() == target.len() && target.iter().all(|column| key.contains(column))
    };
    if keys.iter().any(matches) {
        return Ok(());
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
        "ON CONFLICT ({}) matches no unique key; {available}",
        target.join(", ")
    ))
}

/// Checks that `ON CONFLICT ON CONSTRAINT name` names one of the table's
/// primary key or unique constraints. PostgreSQL accepts only a constraint
/// there, never a unique index.
fn check_conflict_constraint(table: &Table, name: &ObjectName) -> Result<(), String> {
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

struct Context<'a> {
    tables: &'a [Table],
    /// The generated enums, for casts to an enum type.
    enums: &'a [EnumType],
    unsupported: &'a [TableFailure],
    generated_modules: &'a HashSet<String>,
}

fn analyze_one(def: &QueryDef, context: &Context<'_>) -> Result<AnalyzedQuery, String> {
    let fn_name = naming::rust_identifier(&def.name.to_snake_case())?;
    let statements = Parser::parse_sql(&PostgreSqlDialect {}, &def.sql)
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
}

impl<'a, 'c> Analysis<'a, 'c> {
    fn new(context: &'c Context<'a>) -> Self {
        Self {
            context,
            scope: Vec::new(),
            ctes: Vec::new(),
            slots: BTreeMap::new(),
        }
    }

    // ---- statements -------------------------------------------------------

    fn statement(&mut self, statement: &Statement) -> Result<Vec<Output>, String> {
        match statement {
            Statement::Query(query) => self.query(query),
            Statement::Insert(insert) => {
                let TableObject::TableName(name) = &insert.table else {
                    return Err("INSERT into a table function is not supported".to_owned());
                };
                let table = self.resolve_table(name)?;
                let qualifier = insert
                    .table_alias
                    .as_ref()
                    .map(ident_name)
                    .unwrap_or_else(|| table.sql_name.clone());
                self.push_scope(qualifier, TableRef::Schema(table), false);

                let columns: Vec<&Column> = if insert.columns.is_empty() {
                    table.columns.iter().collect()
                } else {
                    insert
                        .columns
                        .iter()
                        .map(|ident| self.table_column(table, &ident_name(ident)))
                        .collect::<Result<_, _>>()?
                };
                let Some(source) = &insert.source else {
                    return Err("INSERT without VALUES is not supported".to_owned());
                };
                let SetExpr::Values(values) = source.body.as_ref() else {
                    return Err("INSERT … SELECT is not supported yet; use VALUES".to_owned());
                };
                for row in &values.rows {
                    if row.len() != columns.len() {
                        return Err(format!(
                            "a VALUES row has {} values for {} columns",
                            row.len(),
                            columns.len()
                        ));
                    }
                    for (column, value) in columns.iter().zip(row) {
                        self.typed_by_column(value, column, column.nullable, "")?;
                    }
                }

                if let Some(OnInsert::OnConflict(on_conflict)) = &insert.on {
                    match &on_conflict.conflict_target {
                        Some(ConflictTarget::Columns(target)) => {
                            check_conflict_target(table, target)?;
                        }
                        Some(ConflictTarget::OnConstraint(name)) => {
                            check_conflict_constraint(table, name)?;
                        }
                        None => {}
                    }
                }
                if let Some(OnInsert::OnConflict(on_conflict)) = &insert.on
                    && let OnConflictAction::DoUpdate(update) = &on_conflict.action
                {
                    self.push_scope("excluded".to_owned(), TableRef::Schema(table), false);
                    for assignment in &update.assignments {
                        self.assignment(table, &assignment.target, &assignment.value)?;
                    }
                    if let Some(selection) = &update.selection {
                        self.expr(selection)?;
                    }
                    self.scope.pop();
                }
                self.returning(insert.returning.as_deref())
            }
            Statement::Update {
                table,
                assignments,
                from,
                selection,
                returning,
                ..
            } => {
                if from.is_some() {
                    return Err("UPDATE … FROM is not supported yet".to_owned());
                }
                if !table.joins.is_empty() {
                    return Err("UPDATE with joins is not supported".to_owned());
                }
                self.scope_table(table)?;
                let TableRef::Schema(target) = self.scope[0].table else {
                    return Err("UPDATE must target a schema table, not a CTE".to_owned());
                };
                for assignment in assignments {
                    self.assignment(target, &assignment.target, &assignment.value)?;
                }
                if let Some(selection) = selection {
                    self.expr(selection)?;
                }
                self.returning(returning.as_deref())
            }
            Statement::Delete(delete) => {
                if delete.using.is_some() {
                    return Err("DELETE … USING is not supported yet".to_owned());
                }
                let (FromTable::WithFromKeyword(from) | FromTable::WithoutKeyword(from)) =
                    &delete.from;
                let [table] = from.as_slice() else {
                    return Err("DELETE must name exactly one table".to_owned());
                };
                if !table.joins.is_empty() {
                    return Err("DELETE with joins is not supported".to_owned());
                }
                self.scope_table(table)?;
                if let Some(selection) = &delete.selection {
                    self.expr(selection)?;
                }
                self.returning(delete.returning.as_deref())
            }
            _ => Err("only SELECT, INSERT, UPDATE, and DELETE statements are supported".to_owned()),
        }
    }

    fn query(&mut self, query: &Query) -> Result<Vec<Output>, String> {
        let visible = self.ctes.len();
        let outputs = self.query_body(query);
        self.ctes.truncate(visible);
        outputs
    }

    fn query_body(&mut self, query: &Query) -> Result<Vec<Output>, String> {
        if let Some(with) = &query.with {
            if with.recursive {
                return Err("WITH RECURSIVE is not supported yet".to_owned());
            }
            for cte in &with.cte_tables {
                self.cte(cte)?;
            }
        }
        let SetExpr::Select(select) = query.body.as_ref() else {
            return Err(
                "only a plain SELECT is supported (no UNION, VALUES, or nesting)".to_owned(),
            );
        };
        let outputs = self.select(select)?;
        match &query.limit_clause {
            Some(LimitClause::LimitOffset { limit, offset, .. }) => {
                if let Some(limit) = limit {
                    self.typed_as(limit, "limit", "i64")?;
                }
                if let Some(offset) = offset {
                    self.typed_as(&offset.value, "offset", "i64")?;
                }
            }
            Some(LimitClause::OffsetCommaLimit { .. }) => {
                return Err(
                    "`LIMIT offset, count` is MySQL syntax; use LIMIT … OFFSET …".to_owned(),
                );
            }
            None => {}
        }
        Ok(outputs)
    }

    /// Analyzes a CTE's query in its own scope and makes its result columns
    /// visible, under the CTE's name, to later CTEs and the main statement.
    fn cte(&mut self, cte: &Cte) -> Result<(), String> {
        let name = ident_name(&cte.alias.name);
        if matches!(
            cte.query.body.as_ref(),
            SetExpr::Insert(_) | SetExpr::Update(_) | SetExpr::Delete(_) | SetExpr::Merge(_)
        ) {
            return Err(format!(
                "data-modifying CTE `{name}` (INSERT, UPDATE, or DELETE inside WITH) is not supported yet"
            ));
        }
        let outer_scope = std::mem::take(&mut self.scope);
        let outputs = self.query(&cte.query);
        self.scope = outer_scope;
        let outputs = outputs?;

        let aliases = &cte.alias.columns;
        if aliases.len() > outputs.len() {
            return Err(format!(
                "CTE `{name}` names {} columns but its query returns {}",
                aliases.len(),
                outputs.len()
            ));
        }
        let mut columns: Vec<Column> = Vec::with_capacity(outputs.len());
        for (index, output) in outputs.into_iter().enumerate() {
            let sql_name = match (aliases.get(index), output.name_error) {
                (Some(alias), _) => ident_name(&alias.name),
                (None, Some(name_error)) => return Err(name_error),
                (None, None) => output.sql_name,
            };
            if columns.iter().any(|column| column.sql_name == sql_name) {
                return Err(format!(
                    "CTE `{name}` returns column `{sql_name}` more than once; alias the duplicates"
                ));
            }
            let Some(rust_type) = output.rust_type else {
                return Err(format!(
                    "cannot infer the type of column `{sql_name}` of CTE `{name}`; cast it explicitly (`(expr)::type AS {sql_name}`)"
                ));
            };
            let (sqlx_type, nullable) = match Shape::of(&rust_type) {
                Shape::Optional(inner) => (inner.to_owned(), true),
                Shape::List(_) | Shape::Value(_) => (rust_type.clone(), false),
            };
            columns.push(Column {
                rust_name: naming::rust_identifier(&sql_name)?,
                sql_name,
                rust_type,
                sqlx_type,
                nullable,
                is_primary_key: false,
                has_default: false,
                is_generated: false,
                value_list: None,
                typed_id: None,
                is_managed: false,
            });
        }
        self.ctes.push(Rc::new(Table {
            schema: None,
            module_name: String::new(),
            struct_name: String::new(),
            sql_name: name,
            columns,
            primary_key: Vec::new(),
            foreign_keys: Vec::new(),
            unique_keys: Vec::new(),
            key_constraints: Vec::new(),
            version_column: None,
            soft_delete_column: None,
            tenant_column: None,
            relationships: Vec::new(),
            before_insert_triggers: Vec::new(),
        }));
        Ok(())
    }

    fn select(&mut self, select: &Select) -> Result<Vec<Output>, String> {
        for table in &select.from {
            self.scope_table(table)?;
        }
        if let Some(selection) = &select.selection {
            self.expr(selection)?;
        }
        if let Some(having) = &select.having {
            self.expr(having)?;
        }
        self.projection(&select.projection)
    }

    fn scope_table(&mut self, table: &TableWithJoins) -> Result<(), String> {
        self.table_factor(&table.relation, false)?;
        for join in &table.joins {
            let (constraint, left_nullable, right_nullable) = match &join.join_operator {
                JoinOperator::Join(constraint) | JoinOperator::Inner(constraint) => {
                    (constraint, false, false)
                }
                JoinOperator::Left(constraint) | JoinOperator::LeftOuter(constraint) => {
                    (constraint, false, true)
                }
                JoinOperator::Right(constraint) | JoinOperator::RightOuter(constraint) => {
                    (constraint, true, false)
                }
                JoinOperator::FullOuter(constraint) => (constraint, true, true),
                JoinOperator::CrossJoin(constraint) => (constraint, false, false),
                other => return Err(format!("join `{other:?}` is not supported")),
            };
            if left_nullable {
                for entry in &mut self.scope {
                    entry.outer_nullable = true;
                }
            }
            self.table_factor(&join.relation, right_nullable)?;
            match constraint {
                JoinConstraint::On(expr) => self.expr(expr)?,
                JoinConstraint::None => {}
                JoinConstraint::Using(_) | JoinConstraint::Natural => {
                    return Err(
                        "JOIN … USING and NATURAL JOIN are not supported; use ON".to_owned()
                    );
                }
            }
        }
        Ok(())
    }

    fn table_factor(&mut self, factor: &TableFactor, outer_nullable: bool) -> Result<(), String> {
        let TableFactor::Table {
            name,
            alias,
            args: None,
            ..
        } = factor
        else {
            return Err(
                "only plain tables are supported in FROM (no subqueries or functions)".to_owned(),
            );
        };
        let table = self.resolve_from(name)?;
        let qualifier = alias
            .as_ref()
            .map(|alias| ident_name(&alias.name))
            .unwrap_or_else(|| table.sql_name.clone());
        if self.scope.iter().any(|entry| entry.qualifier == qualifier) {
            return Err(format!(
                "`{qualifier}` appears twice in FROM; alias one of them"
            ));
        }
        self.push_scope(qualifier, table, outer_nullable);
        Ok(())
    }

    fn push_scope(&mut self, qualifier: String, table: TableRef<'a>, outer_nullable: bool) {
        self.scope.push(ScopeEntry {
            qualifier,
            table,
            outer_nullable,
        });
    }

    fn assignment(
        &mut self,
        table: &Table,
        target: &AssignmentTarget,
        value: &Expr,
    ) -> Result<(), String> {
        let AssignmentTarget::ColumnName(name) = target else {
            return Err("tuple assignments `SET (a, b) = …` are not supported".to_owned());
        };
        let column = self.table_column(table, &object_leaf_name(name))?;
        self.typed_by_column(value, column, column.nullable, "")
    }

    fn returning(&mut self, returning: Option<&[SelectItem]>) -> Result<Vec<Output>, String> {
        match returning {
            Some(items) => self.projection(items),
            None => Ok(Vec::new()),
        }
    }

    // ---- result columns ---------------------------------------------------

    fn projection(&mut self, items: &[SelectItem]) -> Result<Vec<Output>, String> {
        let mut outputs = Vec::new();
        for item in items {
            match item {
                SelectItem::Wildcard(_) => {
                    for entry_index in 0..self.scope.len() {
                        if self.scope[entry_index].qualifier == "excluded" {
                            continue;
                        }
                        self.push_table_outputs(entry_index, &mut outputs);
                    }
                }
                SelectItem::QualifiedWildcard(kind, _) => {
                    let qualifier = match kind {
                        sqlparser::ast::SelectItemQualifiedWildcardKind::ObjectName(name) => {
                            object_leaf_name(name)
                        }
                        sqlparser::ast::SelectItemQualifiedWildcardKind::Expr(expr) => {
                            return Err(format!("`{expr}.*` is not supported"));
                        }
                    };
                    let entry_index = self.scope_entry(&qualifier)?;
                    self.push_table_outputs(entry_index, &mut outputs);
                }
                SelectItem::UnnamedExpr(expr) => {
                    self.expr(expr)?;
                    outputs.push(self.output(expr, None)?);
                }
                SelectItem::ExprWithAlias { expr, alias } => {
                    self.expr(expr)?;
                    outputs.push(self.output(expr, Some(alias))?);
                }
            }
        }
        Ok(outputs)
    }

    fn push_table_outputs(&self, entry_index: usize, outputs: &mut Vec<Output>) {
        let entry = &self.scope[entry_index];
        for (column_index, column) in entry.table.columns.iter().enumerate() {
            outputs.push(Output {
                sql_name: column.sql_name.clone(),
                rust_type: Some(column_type(column, entry.outer_nullable)),
                origin: entry
                    .table
                    .module()
                    .map(|module| (module.to_owned(), column_index)),
                nullable_origin: entry.outer_nullable,
                unknown_reason: None,
                name_error: None,
            });
        }
    }

    fn output(&self, expr: &Expr, alias: Option<&Ident>) -> Result<Output, String> {
        if let Some(reference) = self.column_reference(expr)? {
            let entry = &self.scope[reference.entry];
            let column = &entry.table.columns[reference.column];
            return Ok(Output {
                sql_name: alias
                    .map(ident_name)
                    .unwrap_or_else(|| column.sql_name.clone()),
                rust_type: Some(column_type(column, entry.outer_nullable)),
                origin: entry
                    .table
                    .module()
                    .map(|module| (module.to_owned(), reference.column)),
                nullable_origin: entry.outer_nullable,
                unknown_reason: None,
                name_error: None,
            });
        }

        let (default_name, rust_type) = match expr {
            Expr::Function(function) if is_count(function) => {
                ("count".to_owned(), Some("i64".to_owned()))
            }
            Expr::Function(function) => (object_leaf_name(&function.name), None),
            Expr::Cast { data_type, .. } => (
                String::new(),
                Some(
                    map_type(data_type, self.context.enums)
                        .map(|(_, sqlx_type)| format!("Option<{sqlx_type}>"))?,
                ),
            ),
            _ => (String::new(), None),
        };
        let sql_name = match alias {
            Some(alias) => ident_name(alias),
            None => default_name,
        };
        let name_error = sql_name.is_empty().then(|| {
            format!(
                "result expression `{expr}` needs an alias (`{expr} AS name`) and a `-- column: name <RustType>` override"
            )
        });
        Ok(Output {
            name_error,
            unknown_reason: rust_type.is_none().then(|| {
                format!(
                    "cannot infer the type of result column `{sql_name}` (`{expr}`); add `-- column: {sql_name} <RustType>`"
                )
            }),
            sql_name,
            rust_type,
            origin: None,
            nullable_origin: false,
        })
    }

    // ---- parameters -------------------------------------------------------

    /// Walks an expression, typing every placeholder whose context
    /// determines its type.
    fn expr(&mut self, expr: &Expr) -> Result<(), String> {
        match expr {
            Expr::BinaryOp { left, op, right } => {
                if is_comparison(op) {
                    self.compare(left, right)?;
                    self.compare(right, left)?;
                }
                self.expr(left)?;
                self.expr(right)
            }
            Expr::AnyOp { left, right, .. } | Expr::AllOp { left, right, .. } => {
                if let Some(reference) = self.column_reference(left)?
                    && let Some(index) = placeholder(right)
                {
                    let column = self.reference_column(&reference);
                    let (name, base, raw) = (
                        format!("{}_list", column.rust_name),
                        format!("Vec<{}>", column.value_type()),
                        format!("Vec<{}>", column.sqlx_type),
                    );
                    self.assign(index, &name, &base, &raw, false)?;
                }
                self.expr(left)?;
                self.expr(right)
            }
            Expr::InList { expr, list, .. } => {
                for item in list {
                    self.compare(expr, item)?;
                    self.expr(item)?;
                }
                self.expr(expr)
            }
            Expr::Between {
                expr, low, high, ..
            } => {
                self.compare(expr, low)?;
                self.compare(expr, high)?;
                self.expr(expr)?;
                self.expr(low)?;
                self.expr(high)
            }
            Expr::Like { expr, pattern, .. } | Expr::ILike { expr, pattern, .. } => {
                self.compare(expr, pattern)?;
                self.expr(expr)?;
                self.expr(pattern)
            }
            Expr::Nested(inner)
            | Expr::UnaryOp { expr: inner, .. }
            | Expr::IsNull(inner)
            | Expr::IsNotNull(inner) => self.expr(inner),
            Expr::Cast {
                expr: inner,
                data_type,
                ..
            } => {
                if let Some(index) = placeholder(inner) {
                    let (_, sqlx_type) = map_type(data_type, self.context.enums)?;
                    let name = format!("param_{index}");
                    self.assign(index, &name, &sqlx_type, &sqlx_type, false)?;
                }
                self.expr(inner)
            }
            Expr::Function(function) => {
                if let FunctionArguments::List(list) = &function.args {
                    for argument in &list.args {
                        if let FunctionArg::Unnamed(FunctionArgExpr::Expr(argument)) = argument {
                            self.expr(argument)?;
                        }
                    }
                }
                Ok(())
            }
            Expr::Identifier(_) | Expr::CompoundIdentifier(_) => {
                self.column_reference(expr)?;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Types `value` from `column` when `value` is a placeholder and `column`
    /// a column reference.
    fn compare(&mut self, column: &Expr, value: &Expr) -> Result<(), String> {
        let Some(index) = placeholder_or_cast(value) else {
            return Ok(());
        };
        let Some(reference) = self.column_reference(column)? else {
            return Ok(());
        };
        let column = self.reference_column(&reference).clone();
        self.assign(
            index,
            &column.rust_name,
            column.value_type(),
            &column.sqlx_type,
            false,
        )
    }

    fn typed_by_column(
        &mut self,
        value: &Expr,
        column: &Column,
        nullable: bool,
        suffix: &str,
    ) -> Result<(), String> {
        match placeholder(value) {
            Some(index) => self.assign(
                index,
                &format!("{}{suffix}", column.rust_name),
                column.value_type(),
                &column.sqlx_type,
                nullable,
            ),
            None => self.expr(value),
        }
    }

    fn typed_as(&mut self, value: &Expr, name: &str, rust_type: &str) -> Result<(), String> {
        match placeholder(value) {
            Some(index) => self.assign(index, name, rust_type, rust_type, false),
            None => self.expr(value),
        }
    }

    /// Types placeholder `index` as `base`, whose plain type is `raw` (they
    /// differ for a typed id). A placeholder typed twice keeps one type: the
    /// same type, or the typed id when the other use has its plain type. Two
    /// different typed ids over one plain type fall back to the plain type,
    /// and different plain types fail.
    fn assign(
        &mut self,
        index: usize,
        name: &str,
        base: &str,
        raw: &str,
        nullable: bool,
    ) -> Result<(), String> {
        match self.slots.get_mut(&index) {
            Some(slot) if slot.raw != raw => Err(format!(
                "`${index}` is used both as `{}` and as `{base}`; use separate placeholders or add `-- param: ${index} <name> <RustType>`",
                slot.base
            )),
            Some(slot) => {
                if slot.base != base {
                    if slot.base == slot.raw {
                        slot.base = base.to_owned();
                    } else if base != raw {
                        slot.base = raw.to_owned();
                    }
                }
                slot.nullable &= nullable;
                Ok(())
            }
            None => {
                self.slots.insert(
                    index,
                    Slot {
                        name: name.to_owned(),
                        base: base.to_owned(),
                        raw: raw.to_owned(),
                        nullable,
                    },
                );
                Ok(())
            }
        }
    }

    /// Combines inferred slots with overrides into the final, gap-free
    /// parameter list with unique names.
    fn params(&self, statement: &Statement, def: &QueryDef) -> Result<Vec<Param>, String> {
        let mut used = BTreeSet::new();
        let _ = visit_expressions(statement, |expr| {
            if let Some(index) = placeholder(expr) {
                used.insert(index);
            }
            ControlFlow::<()>::Continue(())
        });

        for override_ in &def.params {
            if !used.contains(&override_.index) {
                return Err(format!(
                    "`-- param: ${}` overrides a placeholder the query does not use",
                    override_.index
                ));
            }
        }

        let highest = used.iter().next_back().copied().unwrap_or(0);
        let mut params = Vec::new();
        let mut names = HashSet::new();
        for index in 1..=highest {
            if !used.contains(&index) {
                return Err(format!(
                    "`${index}` is never used, but `${highest}` is; PostgreSQL cannot infer the type of an unused placeholder"
                ));
            }
            let (name, rust_type) = if let Some(override_) =
                def.params.iter().find(|override_| override_.index == index)
            {
                (override_.name.clone(), override_.rust_type.clone())
            } else if let Some(slot) = self.slots.get(&index) {
                let rust_type = if slot.nullable {
                    format!("Option<{}>", slot.base)
                } else {
                    slot.base.clone()
                };
                (slot.name.clone(), rust_type)
            } else {
                return Err(format!(
                    "cannot infer the type of `${index}` from its context; add `-- param: ${index} <name> <RustType>`"
                ));
            };
            let name = unique_name(&name, &mut names);
            params.push(Param {
                index,
                name,
                rust_type,
            });
        }
        Ok(params)
    }

    // ---- name resolution --------------------------------------------------

    /// Resolves a `FROM` item: an unqualified name matching a visible CTE
    /// names the CTE (shadowing a table of that name), otherwise a schema
    /// table.
    fn resolve_from(&self, name: &ObjectName) -> Result<TableRef<'a>, String> {
        if let [ObjectNamePart::Identifier(ident)] = name.0.as_slice() {
            let leaf = ident_name(ident);
            if let Some(cte) = self.ctes.iter().rev().find(|cte| cte.sql_name == leaf) {
                return Ok(TableRef::Cte(Rc::clone(cte)));
            }
        }
        self.resolve_table(name).map(TableRef::Schema)
    }

    fn resolve_table(&self, name: &ObjectName) -> Result<&'a Table, String> {
        let parts: Vec<String> = name
            .0
            .iter()
            .filter_map(|part| match part {
                ObjectNamePart::Identifier(ident) => Some(ident_name(ident)),
                ObjectNamePart::Function(_) => None,
            })
            .collect();
        let (schema, leaf) = match parts.as_slice() {
            [.., schema, leaf] => (Some(schema.as_str()), leaf.as_str()),
            [leaf] => (None, leaf.as_str()),
            [] => return Err(format!("cannot read table name `{name}`")),
        };
        let candidates: Vec<&Table> = self
            .context
            .tables
            .iter()
            .filter(|table| {
                table.sql_name == leaf
                    && schema.is_none_or(|schema| table.schema.as_deref() == Some(schema))
            })
            .collect();
        match candidates.as_slice() {
            [table] => Ok(table),
            [] => match self
                .context
                .unsupported
                .iter()
                .find(|failure| failure.table == leaf)
            {
                Some(failure) => Err(format!(
                    "table `{leaf}` is not generatable ({}); queries cannot use it",
                    failure.reason
                )),
                None => Err(format!("table `{name}` is not in the schema")),
            },
            _ => Err(format!(
                "table `{leaf}` exists in several schemas; qualify it (`schema.{leaf}`)"
            )),
        }
    }

    fn table_column<'t>(&self, table: &'t Table, name: &str) -> Result<&'t Column, String> {
        table
            .columns
            .iter()
            .find(|column| column.sql_name == name)
            .ok_or_else(|| {
                format!(
                    "column `{name}` does not exist in `{}`",
                    table.display_name()
                )
            })
    }

    fn scope_entry(&self, qualifier: &str) -> Result<usize, String> {
        self.scope
            .iter()
            .position(|entry| entry.qualifier == qualifier)
            .ok_or_else(|| format!("`{qualifier}` is not a table or alias in FROM"))
    }

    /// Resolves a (possibly qualified) column reference. Returns `Ok(None)`
    /// for expressions that are not column references.
    fn column_reference(&self, expr: &Expr) -> Result<Option<Reference>, String> {
        let (qualifier, name) = match expr {
            Expr::Identifier(ident) => (None, ident_name(ident)),
            Expr::CompoundIdentifier(parts) => match parts.as_slice() {
                [.., qualifier, column] => (Some(ident_name(qualifier)), ident_name(column)),
                _ => return Ok(None),
            },
            Expr::Nested(inner) => return self.column_reference(inner),
            _ => return Ok(None),
        };

        if let Some(qualifier) = qualifier {
            let entry = self.scope_entry(&qualifier)?;
            let column = self.scope[entry]
                .table
                .columns
                .iter()
                .position(|column| column.sql_name == name)
                .ok_or_else(|| {
                    format!(
                        "column `{name}` does not exist in `{}`",
                        self.scope[entry].table.display_name()
                    )
                })?;
            return Ok(Some(Reference { entry, column }));
        }

        let matches: Vec<Reference> = self
            .scope
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.qualifier != "excluded")
            .filter_map(|(entry, scope_entry)| {
                scope_entry
                    .table
                    .columns
                    .iter()
                    .position(|column| column.sql_name == name)
                    .map(|column| Reference { entry, column })
            })
            .collect();
        match matches.as_slice() {
            [reference] => Ok(Some(*reference)),
            [] if self.scope.is_empty() => {
                Err(format!("column `{name}` is used without a FROM table"))
            }
            [] => {
                let tables: Vec<String> = self
                    .scope
                    .iter()
                    .filter(|entry| entry.qualifier != "excluded")
                    .map(|entry| format!("`{}`", entry.table.display_name()))
                    .collect();
                Err(format!(
                    "column `{name}` does not exist in {}",
                    tables.join(", ")
                ))
            }
            _ => Err(format!(
                "column `{name}` is ambiguous; qualify it with a table or alias"
            )),
        }
    }

    fn reference_column(&self, reference: &Reference) -> &Column {
        &self.scope[reference.entry].table.columns[reference.column]
    }
}

#[derive(Debug, Clone, Copy)]
struct Reference {
    entry: usize,
    column: usize,
}

fn result_shape(
    def: &QueryDef,
    outputs: Vec<Output>,
    context: &Context<'_>,
) -> Result<ResultShape, String> {
    if let Some(name_error) = outputs.iter().find_map(|output| output.name_error.clone()) {
        return Err(name_error);
    }
    let mut seen = HashSet::new();
    for output in &outputs {
        if !seen.insert(output.sql_name.as_str()) {
            return Err(format!(
                "result column `{}` appears more than once; alias the duplicates",
                output.sql_name
            ));
        }
    }
    for override_ in &def.columns {
        if !seen.contains(override_.name.as_str()) {
            return Err(format!(
                "`-- column: {}` overrides a column the query does not return",
                override_.name
            ));
        }
    }

    if !def.mode.returns_rows() {
        return Ok(ResultShape::None);
    }
    if outputs.is_empty() {
        return Err(format!(
            "`:{}` returns rows, but the statement has no result columns; add RETURNING or use `:exec`",
            def.mode.keyword()
        ));
    }

    let overridden = |name: &str| def.columns.iter().find(|column| column.name == name);
    let mut columns = Vec::with_capacity(outputs.len());
    for output in &outputs {
        let rust_type = match (overridden(&output.sql_name), &output.rust_type) {
            (Some(override_), _) => override_.rust_type.clone(),
            (None, Some(rust_type)) => rust_type.clone(),
            (None, None) => {
                return Err(output.unknown_reason.clone().unwrap_or_else(|| {
                    format!(
                        "cannot infer the type of result column `{}`",
                        output.sql_name
                    )
                }));
            }
        };
        columns.push(OutputColumn {
            rust_name: naming::rust_identifier(&output.sql_name)?,
            sql_name: output.sql_name.clone(),
            rust_type,
        });
    }

    if let Some(model) = reusable_model(def, &outputs, context) {
        return Ok(model);
    }
    if let [column] = columns.as_slice() {
        return Ok(ResultShape::Scalar(column.rust_type.clone()));
    }
    Ok(ResultShape::Row(columns))
}

/// The row struct of a generated resource, when the result is exactly that
/// table's columns in declaration order, unaltered by joins or overrides.
fn reusable_model(
    def: &QueryDef,
    outputs: &[Output],
    context: &Context<'_>,
) -> Option<ResultShape> {
    if !def.columns.is_empty() || outputs.len() < 2 {
        return None;
    }
    let (module, _) = outputs[0].origin.as_ref()?;
    let table = context
        .tables
        .iter()
        .find(|table| &table.module_name == module)?;
    let is_whole_table = outputs.len() == table.columns.len()
        && outputs.iter().enumerate().all(|(index, output)| {
            !output.nullable_origin
                && output.sql_name == table.columns[index].sql_name
                && output.origin.as_ref() == Some(&(module.clone(), index))
        });
    (is_whole_table && context.generated_modules.contains(module)).then(|| ResultShape::Model {
        module: table.module_name.clone(),
        struct_name: table.struct_name.clone(),
    })
}

/// The Rust type of a result column drawn directly from `column`: its field
/// type (the typed id, when it has one), optional on the outer side of a join.
fn column_type(column: &Column, outer_nullable: bool) -> String {
    if outer_nullable && !column.nullable {
        format!("Option<{}>", column.value_type())
    } else {
        column.field_type()
    }
}

fn placeholder(expr: &Expr) -> Option<usize> {
    match expr {
        Expr::Value(value) => match &value.value {
            Value::Placeholder(text) => text.strip_prefix('$')?.parse().ok(),
            _ => None,
        },
        Expr::Nested(inner) => placeholder(inner),
        _ => None,
    }
}

/// A placeholder, possibly cast (`$1::text`): the column it is compared with
/// names it, and the cast and column types must agree.
fn placeholder_or_cast(expr: &Expr) -> Option<usize> {
    match expr {
        Expr::Cast { expr, .. } => placeholder(expr),
        _ => placeholder(expr),
    }
}

fn is_comparison(op: &BinaryOperator) -> bool {
    matches!(
        op,
        BinaryOperator::Eq
            | BinaryOperator::NotEq
            | BinaryOperator::Lt
            | BinaryOperator::LtEq
            | BinaryOperator::Gt
            | BinaryOperator::GtEq
    )
}

fn is_count(function: &sqlparser::ast::Function) -> bool {
    object_leaf_name(&function.name).eq_ignore_ascii_case("count")
}

/// `name`, or `name_2`, `name_3`, … when already taken; also avoids the
/// names generated functions use for their own parameters.
fn unique_name(name: &str, taken: &mut HashSet<String>) -> String {
    let reserved = |candidate: &str| matches!(candidate, "executor" | "params" | "result");
    let mut candidate = name.to_owned();
    let mut counter = 2;
    while reserved(&candidate) || taken.contains(&candidate) {
        candidate = format!("{name}_{counter}");
        counter += 1;
    }
    taken.insert(candidate.clone());
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queries::annotations::parse_query_file;

    const SCHEMA: &str = "
        CREATE TABLE categories (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL);
        CREATE TABLE products (
            id UUID PRIMARY KEY,
            sku TEXT NOT NULL,
            name TEXT NOT NULL,
            category_id BIGINT NOT NULL REFERENCES categories (id),
            note TEXT,
            stock INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE app.tags (id BIGSERIAL PRIMARY KEY, \"select\" TEXT NOT NULL);
        CREATE TABLE spans (id BIGSERIAL PRIMARY KEY, length INET NOT NULL);
        CREATE TYPE product_status AS ENUM ('active', 'retired');
        CREATE TYPE app.ticket_state AS ENUM ('open', 'closed');
        CREATE TABLE gauges (id BIGSERIAL PRIMARY KEY, level TEXT CHECK (level IN ('low', 'high')));
    ";

    fn analyze(queries: &str) -> (Vec<AnalyzedQuery>, Vec<QueryFailure>) {
        let parsed = crate::parser::parse_schema(SCHEMA).unwrap();
        let (defs, failures) = parse_query_file(queries);
        assert!(failures.is_empty(), "{failures:?}");
        let modules = parsed
            .schema
            .tables
            .iter()
            .map(|table| table.module_name.clone())
            .collect();
        analyze_queries(
            &defs,
            &parsed.schema.tables,
            &parsed.schema.enums,
            &modules,
            &parsed.failures,
        )
    }

    fn one(queries: &str) -> AnalyzedQuery {
        let (mut analyzed, failures) = analyze(queries);
        assert!(failures.is_empty(), "{failures:?}");
        analyzed.remove(0)
    }

    fn failure(queries: &str) -> String {
        let (analyzed, failures) = analyze(queries);
        assert!(analyzed.is_empty(), "expected a failure, got {analyzed:?}");
        failures[0].reason.clone()
    }

    fn param(index: usize, name: &str, rust_type: &str) -> Param {
        Param {
            index,
            name: name.to_owned(),
            rust_type: rust_type.to_owned(),
        }
    }

    #[test]
    fn infers_params_from_comparisons_and_limits() {
        let query = one(
            "-- name: ListProducts :many\nSELECT id, sku FROM products WHERE category_id = $1 AND stock <= $2 ORDER BY sku LIMIT $3 OFFSET $4;",
        );
        assert_eq!(query.fn_name, "list_products");
        assert_eq!(
            query.params,
            [
                param(1, "category_id", "i64"),
                param(2, "stock", "i32"),
                param(3, "limit", "i64"),
                param(4, "offset", "i64"),
            ]
        );
        assert_eq!(
            query.sql,
            "SELECT id, sku FROM products WHERE category_id = $1 AND stock <= $2 ORDER BY sku LIMIT $3 OFFSET $4"
        );
        let ResultShape::Row(columns) = &query.result else {
            panic!("expected a row struct, got {:?}", query.result);
        };
        assert_eq!(columns[0].rust_type, "uuid::Uuid");
        assert_eq!(columns[1].rust_name, "sku");
    }

    #[test]
    fn casts_to_interval_bytea_and_timetz_infer_their_mapped_types() {
        let query = one(
            "-- name: Q :many\nSELECT id, '1 day'::interval AS span, sku::bytea AS raw, '12:00+01'::timetz AS opens \
             FROM products WHERE $1::interval IS NOT NULL AND $2::bytea IS NOT NULL AND $3::time with time zone IS NOT NULL;",
        );
        let interval = "sqlx::postgres::types::PgInterval";
        let time_tz = "sqlx::postgres::types::PgTimeTz";
        assert_eq!(
            query.params,
            [
                param(1, "param_1", interval),
                param(2, "param_2", "Vec<u8>"),
                param(3, "param_3", time_tz),
            ]
        );
        let ResultShape::Row(columns) = &query.result else {
            panic!("expected a row struct");
        };
        let types: Vec<&str> = columns
            .iter()
            .map(|column| column.rust_type.as_str())
            .collect();
        assert_eq!(
            types[1..],
            [
                format!("Option<{interval}>"),
                "Option<Vec<u8>>".to_owned(),
                format!("Option<{time_tz}>"),
            ]
        );
    }

    #[test]
    fn infers_params_from_in_any_between_like_and_casts() {
        let query = one(
            "-- name: Search :many\nSELECT id, sku FROM products WHERE sku LIKE $1 AND stock BETWEEN $2 AND $3 AND category_id IN ($4, $5) AND id = ANY($6) AND name = $7::text;",
        );
        assert_eq!(
            query.params,
            [
                param(1, "sku", "String"),
                param(2, "stock", "i32"),
                param(3, "stock_2", "i32"),
                param(4, "category_id", "i64"),
                param(5, "category_id_2", "i64"),
                param(6, "id_list", "Vec<uuid::Uuid>"),
                param(7, "name", "String"),
            ]
        );
    }

    #[test]
    fn insert_values_take_column_types_and_nullability() {
        let query = one(
            "-- name: CreateProducts :execrows\nINSERT INTO products (id, sku, name, category_id, note) VALUES ($1, $2, $3, $4, $5), ($6, $7, $8, $9, NULL);",
        );
        assert_eq!(query.params[4], param(5, "note", "Option<String>"));
        assert_eq!(query.params[5], param(6, "id_2", "uuid::Uuid"));
        assert_eq!(query.params.len(), 9);
        assert_eq!(query.result, ResultShape::None);
    }

    #[test]
    fn update_set_and_upsert_assignments_are_typed() {
        let query =
            one("-- name: Rename :exec\nUPDATE products SET name = $2, note = $3 WHERE id = $1;");
        assert_eq!(
            query.params,
            [
                param(1, "id", "uuid::Uuid"),
                param(2, "name", "String"),
                param(3, "note", "Option<String>"),
            ]
        );
        let upsert = one(
            "-- name: Upsert :exec\nINSERT INTO products (id, sku, name, category_id) VALUES ($1, $2, $3, $4)\nON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, stock = $5;",
        );
        assert_eq!(upsert.params[4], param(5, "stock", "i32"));
    }

    #[test]
    fn returning_star_reuses_the_resource_model() {
        let query = one(
            "-- name: CreateCategory :one\nINSERT INTO categories (name) VALUES ($1) RETURNING *;",
        );
        assert_eq!(
            query.result,
            ResultShape::Model {
                module: "categories".to_owned(),
                struct_name: "Category".to_owned()
            }
        );
    }

    #[test]
    fn casts_to_enum_types_infer_the_generated_enum() {
        let query = one(
            "-- name: Statuses :many\nSELECT id FROM products WHERE $1::product_status IS NOT NULL AND $2::app.ticket_state IS NOT NULL AND $3::product_status[] IS NOT NULL AND $4::public.product_status IS NOT NULL;",
        );
        assert_eq!(
            query.params,
            [
                param(1, "param_1", "types::ProductStatus"),
                param(2, "param_2", "types::TicketState"),
                param(3, "param_3", "Vec<types::ProductStatus>"),
                param(4, "param_4", "types::ProductStatus"),
            ]
        );
        let status =
            one("-- name: Status :one\nSELECT 'active'::product_status AS status FROM products;");
        assert_eq!(
            status.result,
            ResultShape::Scalar("Option<types::ProductStatus>".to_owned())
        );
    }

    /// An enum type is found under its schema, and a value-list enum is no
    /// database type at all.
    #[test]
    fn casts_to_other_names_are_not_enums() {
        for cast in ["ticket_state", "app.product_status", "level"] {
            let reason = failure(&format!(
                "-- name: Q :many\nSELECT id FROM gauges WHERE $1::{cast} IS NOT NULL;"
            ));
            assert!(reason.contains("is not supported yet"), "{cast}: {reason}");
        }
    }

    #[test]
    fn partial_selects_get_row_structs_and_single_columns_scalars() {
        let row = one("-- name: Names :many\nSELECT name, id FROM categories;");
        assert!(matches!(row.result, ResultShape::Row(_)));
        let scalar = one("-- name: CountProducts :one\nSELECT COUNT(*) FROM products;");
        assert_eq!(scalar.result, ResultShape::Scalar("i64".to_owned()));
        let note = one("-- name: Note :maybe_one\nSELECT note FROM products WHERE id = $1;");
        assert_eq!(
            note.result,
            ResultShape::Scalar("Option<String>".to_owned())
        );
    }

    #[test]
    fn left_joins_make_the_right_side_nullable() {
        let query = one(
            "-- name: WithCategory :many\nSELECT p.id, p.sku, c.name AS category_name FROM products p LEFT JOIN categories c ON c.id = p.category_id WHERE c.name = $1;",
        );
        let ResultShape::Row(columns) = &query.result else {
            panic!("expected a row struct");
        };
        assert_eq!(columns[2].sql_name, "category_name");
        assert_eq!(columns[2].rust_type, "Option<String>");
        assert_eq!(query.params, [param(1, "name", "String")]);
    }

    #[test]
    fn schema_qualified_and_quoted_names_resolve() {
        let query =
            one("-- name: Tags :many\nSELECT id, \"select\" FROM app.tags WHERE \"select\" = $1;");
        assert_eq!(
            query.result,
            ResultShape::Model {
                module: "tags".to_owned(),
                struct_name: "Tag".to_owned()
            }
        );
        assert_eq!(query.params, [param(1, "select", "String")]);
    }

    #[test]
    fn overrides_supply_types_inference_cannot() {
        let query = one(
            "-- name: Totals :many\n-- param: $1 min_total i64\n-- column: total i64\nSELECT category_id, SUM(stock)::bigint AS total FROM products GROUP BY category_id HAVING SUM(stock) > $1;",
        );
        assert_eq!(query.params, [param(1, "min_total", "i64")]);
        let ResultShape::Row(columns) = &query.result else {
            panic!("expected a row struct");
        };
        assert_eq!(columns[1].rust_type, "i64");
    }

    #[test]
    fn reports_untypable_and_unused_placeholders() {
        assert!(
            failure("-- name: Q :many\nSELECT id FROM products WHERE lower(name) = lower($1);")
                .contains("add `-- param: $1 <name> <RustType>`")
        );
        assert!(
            failure("-- name: Q :many\nSELECT id FROM products WHERE sku = $2;")
                .contains("`$1` is never used")
        );
        assert!(
            failure("-- name: Q :many\nSELECT id FROM products WHERE sku = $1 OR stock = $1;")
                .contains("used both as `String` and as `i32`")
        );
    }

    #[test]
    fn reports_schema_mismatches() {
        assert!(
            failure("-- name: Q :many\nSELECT id FROM nope;")
                .contains("`nope` is not in the schema")
        );
        assert!(
            failure("-- name: Q :many\nSELECT colour FROM products;")
                .contains("column `colour` does not exist in `products`")
        );
        assert!(
            failure(
                "-- name: Q :many\nSELECT id, name FROM products p JOIN categories c ON c.id = p.category_id;"
            )
            .contains("ambiguous")
        );
        assert!(
            failure("-- name: Q :many\nSELECT id FROM spans;")
                .contains("table `spans` is not generatable")
        );
    }

    #[test]
    fn reports_unsupported_statement_shapes() {
        assert!(
            failure("-- name: Q :exec\nBEGIN;\nDELETE FROM products;\nCOMMIT;")
                .contains("contains 3 statements")
        );
        assert!(
            failure("-- name: Q :one\nDELETE FROM products WHERE id = $1;")
                .contains("add RETURNING or use `:exec`")
        );
        assert!(
            failure("-- name: Q :many\nSELECT upper(name) FROM products;")
                .contains("`-- column: upper <RustType>`")
        );
        assert!(
            failure("-- name: Q :many\nSELECT id, id FROM products;")
                .contains("appears more than once")
        );
        assert!(failure("-- name: Q :many\nSELECT 1 FROM products;").contains("needs an alias"));
    }

    fn row_columns(query: &AnalyzedQuery) -> Vec<(&str, &str)> {
        let ResultShape::Row(columns) = &query.result else {
            panic!("expected a row struct, got {:?}", query.result);
        };
        columns
            .iter()
            .map(|column| (column.sql_name.as_str(), column.rust_type.as_str()))
            .collect()
    }

    #[test]
    fn cte_results_join_like_tables_and_type_their_placeholders() {
        let query = one("-- name: StockByCategory :many\n\
             WITH stock AS (\n\
                 SELECT category_id, SUM(stock)::bigint AS total FROM products WHERE stock >= $1 GROUP BY category_id\n\
             )\n\
             SELECT c.id, c.name, s.total FROM categories c JOIN stock s ON s.category_id = c.id WHERE s.total > $2;");
        assert_eq!(
            row_columns(&query),
            [("id", "i64"), ("name", "String"), ("total", "Option<i64>")]
        );
        assert_eq!(
            query.params,
            [param(1, "stock", "i32"), param(2, "total", "i64")]
        );
    }

    #[test]
    fn later_ctes_see_earlier_ones_and_column_lists_rename() {
        let query = one("-- name: BigCategories :many\n\
             WITH counted (cat, n) AS (SELECT category_id, COUNT(*) FROM products GROUP BY category_id),\n\
                  big AS (SELECT cat, n AS product_count FROM counted WHERE n > $1)\n\
             SELECT big.cat, big.product_count, c.name FROM big JOIN categories c ON c.id = big.cat;");
        assert_eq!(
            row_columns(&query),
            [("cat", "i64"), ("product_count", "i64"), ("name", "String")]
        );
        assert_eq!(query.params, [param(1, "n", "i64")]);
    }

    #[test]
    fn a_cte_column_list_names_unaliased_expressions() {
        let query = one(
            "-- name: Totals :many\nWITH t (category_id, total) AS (SELECT category_id, SUM(stock)::bigint FROM products GROUP BY category_id) SELECT category_id, total FROM t;",
        );
        assert_eq!(
            row_columns(&query),
            [("category_id", "i64"), ("total", "Option<i64>")]
        );
        assert!(
            failure("-- name: Q :many\nWITH t AS (SELECT SUM(stock)::bigint FROM products) SELECT * FROM t;")
                .contains("needs an alias")
        );
    }

    #[test]
    fn a_cte_selecting_a_whole_table_does_not_reuse_its_model() {
        let query =
            one("-- name: Cats :many\nWITH cats AS (SELECT * FROM categories) SELECT * FROM cats;");
        assert_eq!(row_columns(&query), [("id", "i64"), ("name", "String")]);
    }

    #[test]
    fn ctes_are_scoped_to_their_statement_and_must_be_typable() {
        assert!(
            failure("-- name: Q :many\nWITH a AS (SELECT id FROM b), b AS (SELECT id FROM categories) SELECT id FROM a;")
                .contains("table `b` is not in the schema")
        );
        assert!(
            failure("-- name: Q :many\nWITH a AS (SELECT SUM(stock) AS total FROM products) SELECT total FROM a;")
                .contains("cannot infer the type of column `total` of CTE `a`")
        );
        assert!(
            failure(
                "-- name: Q :many\nWITH a (x, y) AS (SELECT id FROM categories) SELECT x FROM a;"
            )
            .contains("CTE `a` names 2 columns but its query returns 1")
        );
    }

    #[test]
    fn recursive_and_data_modifying_ctes_are_rejected() {
        assert!(
            failure(
                "-- name: Q :many\nWITH RECURSIVE t (n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM t) SELECT n FROM t;"
            )
            .contains("WITH RECURSIVE is not supported")
        );
        assert!(
            failure(
                "-- name: Q :many\nWITH gone AS (DELETE FROM products WHERE stock = $1 RETURNING id) SELECT id FROM gone;"
            )
            .contains("data-modifying CTE `gone`")
        );
    }
}
