//! Name resolution: tables, CTEs, and column references in scope.

use std::rc::Rc;

use sqlparser::ast::{Expr, ObjectName, ObjectNamePart};

use super::{Analysis, TableRef};
use crate::parser::postgres::ident_name;
use crate::schema::{Column, Table};

impl<'a, 'c> Analysis<'a, 'c> {
    /// Resolves a `FROM` item: an unqualified name matching a visible CTE
    /// names the CTE (shadowing a table of that name), otherwise a schema
    /// table.
    pub(super) fn resolve_from(&self, name: &ObjectName) -> Result<TableRef<'a>, String> {
        if let [ObjectNamePart::Identifier(ident)] = name.0.as_slice() {
            let leaf = ident_name(ident);
            if let Some(cte) = self.ctes.iter().rev().find(|cte| cte.sql_name == leaf) {
                return Ok(TableRef::Cte(Rc::clone(cte)));
            }
        }
        self.resolve_table(name).map(TableRef::Schema)
    }

    pub(super) fn resolve_table(&self, name: &ObjectName) -> Result<&'a Table, String> {
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

    pub(super) fn table_column<'t>(
        &self,
        table: &'t Table,
        name: &str,
    ) -> Result<&'t Column, String> {
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

    pub(super) fn scope_entry(&self, qualifier: &str) -> Result<usize, String> {
        self.scope
            .iter()
            .position(|entry| entry.qualifier == qualifier)
            .ok_or_else(|| format!("`{qualifier}` is not a table or alias in FROM"))
    }

    /// Resolves a (possibly qualified) column reference. Returns `Ok(None)`
    /// for expressions that are not column references.
    pub(super) fn column_reference(&self, expr: &Expr) -> Result<Option<Reference>, String> {
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

    pub(super) fn reference_column(&self, reference: &Reference) -> &Column {
        &self.scope[reference.entry].table.columns[reference.column]
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Reference {
    pub(super) entry: usize,
    pub(super) column: usize,
}
