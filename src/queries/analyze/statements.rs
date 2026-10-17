//! Statements, CTEs, and the tables each brings into scope.

use std::rc::Rc;

use sqlparser::ast::{
    AssignmentTarget, ConflictTarget, Cte, Expr, FromTable, JoinConstraint, JoinOperator,
    LimitClause, OnConflictAction, OnInsert, Query, Select, SelectItem, SetExpr, SetOperator,
    Statement, TableFactor, TableObject, TableWithJoins,
};

use super::conflict::{check_conflict_constraint, check_conflict_target};
use super::{Analysis, Output, ScopeEntry, TableRef};
use crate::parser::naming;
use crate::parser::postgres::{ident_name, object_leaf_name};
use crate::schema::{Column, Shape, Table};

impl<'a, 'c> Analysis<'a, 'c> {
    pub(super) fn statement(&mut self, statement: &Statement) -> Result<Vec<Output>, String> {
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
                            check_conflict_target(table, target, self.conflict_predicate.as_ref())?;
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
            for cte in &with.cte_tables {
                match cte.query.body.as_ref() {
                    SetExpr::SetOperation {
                        op: SetOperator::Union,
                        left,
                        right,
                        ..
                    } if with.recursive => self.recursive_cte(cte, left, right)?,
                    _ => self.cte(cte)?,
                }
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
        let table = self.cte_table(cte, outputs?)?;
        self.ctes.push(Rc::new(table));
        Ok(())
    }

    /// Analyzes a recursive CTE, `anchor UNION [ALL] recursive-term`: the
    /// anchor's result columns give the CTE's, and the recursive term, which
    /// sees the CTE, must return as many columns of the same types. A
    /// nullable column of either term makes the CTE's column nullable.
    fn recursive_cte(&mut self, cte: &Cte, anchor: &SetExpr, term: &SetExpr) -> Result<(), String> {
        let name = ident_name(&cte.alias.name);
        let anchor = self.term(anchor)?;
        let mut table = self.cte_table(cte, anchor)?;
        self.ctes.push(Rc::new(table.clone()));
        let outputs = self.term(term);
        self.ctes.pop();
        let outputs = outputs?;
        if outputs.len() != table.columns.len() {
            return Err(format!(
                "the recursive term of CTE `{name}` returns {} columns but its anchor returns {}",
                outputs.len(),
                table.columns.len()
            ));
        }
        for (column, output) in table.columns.iter_mut().zip(outputs) {
            let Some(rust_type) = output.rust_type else {
                continue;
            };
            let (base, nullable) = match Shape::of(&rust_type) {
                Shape::Optional(inner) => (inner, true),
                Shape::List(_) | Shape::Value(_) => (rust_type.as_str(), false),
            };
            if base != column.sqlx_type {
                return Err(format!(
                    "the recursive term of CTE `{name}` returns `{base}` for column `{}`, but its anchor returns `{}`; cast one of them",
                    column.sql_name, column.sqlx_type
                ));
            }
            if nullable && !column.nullable {
                column.nullable = true;
                column.rust_type = format!("Option<{}>", column.sqlx_type);
            }
        }
        self.ctes.push(Rc::new(table));
        Ok(())
    }

    /// One term of a set operation, analyzed in its own scope.
    fn term(&mut self, body: &SetExpr) -> Result<Vec<Output>, String> {
        let outer_scope = std::mem::take(&mut self.scope);
        let outputs = match body {
            SetExpr::Select(select) => self.select(select),
            SetExpr::Query(query) => self.query(query),
            _ => Err(
                "a recursive CTE's terms must each be a plain SELECT (no nested UNION or VALUES)"
                    .to_owned(),
            ),
        };
        self.scope = outer_scope;
        outputs
    }

    /// The table a CTE's result columns make, named by its column list or
    /// after its query's columns.
    fn cte_table(&self, cte: &Cte, outputs: Vec<Output>) -> Result<Table, String> {
        let name = ident_name(&cte.alias.name);
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
                declared_length: None,
                value_range: None,
                length_range: None,
                typed_id: None,
                is_managed: false,
                id_generation: None,
            });
        }
        Ok(Table {
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
        })
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
}
