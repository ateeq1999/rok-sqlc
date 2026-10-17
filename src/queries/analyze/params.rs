//! Placeholder typing: the contexts and casts that determine a parameter's
//! type.

use std::collections::{BTreeSet, HashSet};
use std::ops::ControlFlow;

use sqlparser::ast::{
    BinaryOperator, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, Statement, Value,
    visit_expressions,
};

use super::{Analysis, Param, Slot};
use crate::parser::postgres::map_type;
use crate::queries::annotations::QueryDef;
use crate::schema::Column;

impl<'a, 'c> Analysis<'a, 'c> {
    /// Walks an expression, typing every placeholder whose context
    /// determines its type.
    pub(super) fn expr(&mut self, expr: &Expr) -> Result<(), String> {
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

    pub(super) fn typed_by_column(
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

    pub(super) fn typed_as(
        &mut self,
        value: &Expr,
        name: &str,
        rust_type: &str,
    ) -> Result<(), String> {
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
    pub(super) fn params(
        &self,
        statement: &Statement,
        def: &QueryDef,
    ) -> Result<Vec<Param>, String> {
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
