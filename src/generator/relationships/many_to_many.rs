//! Join tables and the many-to-many relationships through them.

use super::find_column;
use crate::schema::{Column, Relationship, RelationshipKind, Table, Through};

/// A join table's two foreign keys (ticket 23): together, apart from the
/// tenant column, they form a unique key, and each is a belongs-to to a
/// different table, neither of them the join table itself. Both columns are
/// `NOT NULL`, so every row pairs two rows.
pub struct JoinPair<'a> {
    /// The unique key's first non-tenant column, and its belongs-to.
    pub near: &'a Column,
    pub near_relationship: &'a Relationship,
    /// The unique key's second non-tenant column, and its belongs-to.
    pub far: &'a Column,
    pub far_relationship: &'a Relationship,
    /// The unique key's columns as declared, the tenant column included when
    /// the key has it: the conflict target of an insert.
    pub key: &'a [String],
    /// Whether the key is a unique index over live rows only
    /// (`WHERE <soft-delete column> IS NULL`).
    pub partial: bool,
}

/// The join pair of `table` when it is a join table, read from its
/// belongs-to relationships: the first unique key (primary key first) whose
/// columns, leaving out the tenant column, are two such foreign keys.
pub fn join_pair(table: &Table) -> Option<JoinPair<'_>> {
    let soft_delete = table.soft_delete_column.as_deref();
    let primary = (!table.primary_key.is_empty()).then_some((&table.primary_key, false));
    let declared = table
        .unique_keys
        .iter()
        .filter(|key| key.counts(soft_delete))
        .map(|key| (&key.columns, key.predicate.is_some()));
    primary
        .into_iter()
        .chain(declared)
        .find_map(|(key, partial)| {
            let pair: Vec<&String> = key
                .iter()
                .filter(|name| table.tenant_column.as_deref() != Some(name.as_str()))
                .collect();
            let [near, far] = pair.as_slice() else {
                return None;
            };
            let near_relationship = belongs_to(table, near)?;
            let far_relationship = belongs_to(table, far)?;
            let targets = [&near_relationship.target, &far_relationship.target];
            if targets[0] == targets[1] || targets.contains(&&table.module_name) {
                return None;
            }
            let near = find_column(table, near).filter(|column| !column.nullable)?;
            let far = find_column(table, far).filter(|column| !column.nullable)?;
            Some(JoinPair {
                near,
                near_relationship,
                far,
                far_relationship,
                key,
                partial,
            })
        })
}

fn belongs_to<'a>(table: &'a Table, column: &str) -> Option<&'a Relationship> {
    table.relationships.iter().find(|relationship| {
        relationship.kind == RelationshipKind::BelongsTo && relationship.column == column
    })
}

/// The many-to-many relationships each table gains from the join tables
/// among `tables`, whose belongs-to relationships are already inferred. Each
/// is named after the related table's module (`permissions`), or adds the
/// join table (`permissions_through_role_permissions`) when the table
/// already has a relationship of that name or several join tables lead to
/// the same table.
pub(super) fn many_to_many(tables: &[Table]) -> Vec<Vec<Relationship>> {
    let mut gained: Vec<Vec<Relationship>> = vec![Vec::new(); tables.len()];
    for join in tables {
        let Some(pair) = join_pair(join) else {
            continue;
        };
        let sides = [
            (pair.near_relationship, pair.far_relationship),
            (pair.far_relationship, pair.near_relationship),
        ];
        for (from, to) in sides {
            let index = tables
                .iter()
                .position(|table| table.module_name == from.target)
                .expect("a belongs-to's target is generated with it");
            gained[index].push(Relationship {
                name: to.target.clone(),
                kind: RelationshipKind::ManyToMany,
                target: to.target.clone(),
                column: from.target_column.clone(),
                target_column: to.target_column.clone(),
                through: Some(Through {
                    table: join.module_name.clone(),
                    column: from.column.clone(),
                    target_column: to.column.clone(),
                }),
            });
        }
    }
    for (table, relationships) in tables.iter().zip(&mut gained) {
        let names: Vec<String> = relationships
            .iter()
            .map(|relationship| {
                let parallel = relationships
                    .iter()
                    .filter(|other| other.target == relationship.target)
                    .count();
                let taken = table
                    .relationships
                    .iter()
                    .any(|other| other.name == relationship.target);
                if parallel > 1 || taken {
                    let through = relationship.through.as_ref().expect("a join table");
                    format!("{}_through_{}", relationship.target, through.table)
                } else {
                    relationship.target.clone()
                }
            })
            .collect();
        for (relationship, name) in relationships.iter_mut().zip(names) {
            relationship.name = name;
        }
    }
    gained
}
