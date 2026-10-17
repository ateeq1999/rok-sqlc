//! Result columns and the Rust shape of a query's result.

use std::collections::HashSet;

use sqlparser::ast::{Expr, Ident, SelectItem};

use super::{Analysis, Context, Output, OutputColumn, ResultShape};
use crate::parser::naming;
use crate::parser::postgres::{ident_name, map_type, object_leaf_name};
use crate::queries::annotations::QueryDef;
use crate::schema::Column;

impl<'a, 'c> Analysis<'a, 'c> {
    pub(super) fn projection(&mut self, items: &[SelectItem]) -> Result<Vec<Output>, String> {
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
}

pub(super) fn result_shape(
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

fn is_count(function: &sqlparser::ast::Function) -> bool {
    object_leaf_name(&function.name).eq_ignore_ascii_case("count")
}
