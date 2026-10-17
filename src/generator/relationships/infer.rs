//! Inference: the foreign keys that make relationships, and the names each
//! side gets.

use heck::ToSnakeCase;

use crate::parser::naming::rust_identifier;
use crate::schema::{Column, Relationship, RelationshipKind, Table};

use super::find_column;
use super::many_to_many::many_to_many;
use crate::generator::keys;

/// One foreign key that makes a relationship, with its two tables.
struct Edge {
    child: usize,
    parent: usize,
    /// The foreign key column (SQL name).
    column: String,
    /// The referenced column (SQL name).
    parent_column: String,
    /// The belongs-to name.
    name: String,
    /// Whether the foreign key column alone is a unique key of the child.
    unique: bool,
}

/// Infers the relationships of `tables` and records them, replacing any
/// recorded before.
pub(super) fn record(tables: &mut [Table]) {
    let edges = edges(tables);
    let mut inferred: Vec<Vec<Relationship>> = vec![Vec::new(); tables.len()];
    for edge in &edges {
        inferred[edge.child].push(Relationship {
            name: edge.name.clone(),
            kind: RelationshipKind::BelongsTo,
            target: tables[edge.parent].module_name.clone(),
            column: edge.column.clone(),
            target_column: edge.parent_column.clone(),
            through: None,
        });
    }
    for edge in &edges {
        let child = &tables[edge.child];
        let kind = if edge.unique {
            RelationshipKind::HasOne
        } else {
            RelationshipKind::HasMany
        };
        inferred[edge.parent].push(Relationship {
            name: inverse_name(edge, &edges, child, kind),
            kind,
            target: child.module_name.clone(),
            column: edge.parent_column.clone(),
            target_column: edge.column.clone(),
            through: None,
        });
    }
    for (table, relationships) in tables.iter_mut().zip(inferred) {
        table.relationships = relationships;
    }
    let many_to_many = many_to_many(tables);
    for (table, relationships) in tables.iter_mut().zip(many_to_many) {
        table.relationships.extend(relationships);
    }
}

fn edges(tables: &[Table]) -> Vec<Edge> {
    let mut edges = Vec::new();
    for (child_index, child) in tables.iter().enumerate() {
        for foreign_key in &child.foreign_keys {
            if child.tenant_column.as_deref() == Some(foreign_key.column.as_str()) {
                continue;
            }
            let Some(parent) = keys::referenced_table(tables, foreign_key) else {
                continue;
            };
            let parent_index = tables
                .iter()
                .position(|table| std::ptr::eq(table, parent))
                .expect("the referenced table is one of `tables`");
            let (Some(column), Some(parent_column)) = (
                find_column(child, &foreign_key.column),
                find_column(parent, &foreign_key.referenced_column),
            ) else {
                continue;
            };
            if column.sqlx_type != parent_column.sqlx_type || !column.supports_typed_id() {
                continue;
            }
            edges.push(Edge {
                child: child_index,
                parent: parent_index,
                column: column.sql_name.clone(),
                parent_column: parent_column.sql_name.clone(),
                name: belongs_to_name(column, parent_column, parent),
                unique: is_unique(child, column),
            });
        }
    }
    edges
}

/// The column without `_id` (`category_id` → `category`), or without the
/// referenced column's name (`currency_code` referencing `code` →
/// `currency`); a column named like the referenced one gives the referenced
/// row's name (`user`), and any other column keeps its name.
fn belongs_to_name(column: &Column, parent_column: &Column, parent: &Table) -> String {
    let name = column.sql_name.to_snake_case();
    let referenced = parent_column.sql_name.to_snake_case();
    if name == referenced {
        return parent.struct_name.to_snake_case();
    }
    let stripped = name
        .strip_suffix("_id")
        .or_else(|| name.strip_suffix(&format!("_{referenced}")))
        .filter(|stripped| !stripped.is_empty());
    match stripped {
        Some(stripped) => rust_identifier(stripped).unwrap_or_else(|_| stripped.to_owned()),
        None => column.rust_name.clone(),
    }
}

/// Whether `column` alone is a unique key of `table`, leaving out the
/// tenant column: within one tenant, a key over it and the column is unique.
fn is_unique(table: &Table, column: &Column) -> bool {
    table
        .unique_key_columns(table.soft_delete_column.as_deref())
        .iter()
        .any(|key| {
            let rest: Vec<&String> = key
                .iter()
                .filter(|name| table.tenant_column.as_deref() != Some(name.as_str()))
                .collect();
            rest == [&column.sql_name]
        })
}

/// The inverse's name: the child module for a has-many (`products`), the
/// child row for a has-one (`user_profile`); `children` and `child` through
/// a `parent_id` self-reference. A child with several foreign keys to the
/// parent, and any other self-reference, adds the belongs-to name
/// (`invoices_by_created_by_user`).
fn inverse_name(edge: &Edge, edges: &[Edge], child: &Table, kind: RelationshipKind) -> String {
    let self_reference = edge.child == edge.parent;
    if self_reference && edge.column == "parent_id" {
        return if kind.is_to_many() {
            "children"
        } else {
            "child"
        }
        .to_owned();
    }
    let base = if kind.is_to_many() {
        child.module_name.clone()
    } else {
        child.struct_name.to_snake_case()
    };
    let siblings = edges
        .iter()
        .filter(|other| other.child == edge.child && other.parent == edge.parent)
        .count();
    if self_reference || siblings > 1 {
        format!("{base}_by_{}", edge.name)
    } else {
        base
    }
}
