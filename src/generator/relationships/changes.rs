//! The `[relationships]` configuration: renames and opt-outs, checked and
//! applied, and the name collisions they can cause.

use std::collections::BTreeMap;

use super::{Changes, Scope};
use crate::generator::prepare;
use crate::parser::naming::rust_identifier;
use crate::schema::{Relationship, RelationshipKind, Table};
use crate::{Conventions, RelationshipChange};

/// Checks `changes` against the relationships inferred over every table of
/// the schema, not just the ones a run generates (`--resource`): a change
/// naming a table or relationship that does not exist, an invalid new name,
/// or a rename that collides fails, naming the configuration entry.
pub fn check_changes(
    tables: &[Table],
    conventions: &Conventions,
    changes: &Changes,
) -> Result<(), String> {
    if changes.is_empty() {
        return Ok(());
    }
    prepare(tables, conventions, changes, Scope::Schema).map(drop)
}

/// Where a `[relationships]` entry lives, as a dotted key path:
/// `relationships.invoices.created_by_user`.
pub(super) fn location(table: &str, name: Option<&str>) -> String {
    let quote = |key: &str| {
        if key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            key.to_owned()
        } else {
            format!("\"{key}\"")
        }
    };
    match name {
        Some(name) => format!("`relationships.{}.{}`", quote(table), quote(name)),
        None => format!("`relationships.{}`", quote(table)),
    }
}

/// New names given by configuration, by table module and new name, with the
/// configuration entries that give them.
type Renamed = BTreeMap<(String, String), Vec<String>>;

/// Applies `changes` to the inferred relationships of `tables`: a rename
/// changes the relationship's name, an opt-out removes it and its other side.
/// With `strict`, an entry naming a table or relationship not among `tables`
/// is an error; otherwise it is skipped.
pub(super) fn apply_changes(
    tables: &mut [Table],
    changes: &Changes,
    strict: bool,
) -> Result<Renamed, String> {
    // (table index, relationship index) of every relationship opted out, with
    // the entry that opts it out.
    let mut removed: BTreeMap<(usize, usize), String> = BTreeMap::new();
    let mut renames: Vec<(usize, usize, String, String)> = Vec::new();
    for (key, entries) in changes {
        let Some(table_index) = tables.iter().position(|table| {
            table.display_name() == *key || table.module_name == *key || table.sql_name == *key
        }) else {
            if strict {
                return Err(format!(
                    "{} in the configuration names no table of the schema",
                    location(key, None)
                ));
            }
            continue;
        };
        let table = &tables[table_index];
        for (name, change) in entries {
            let at = location(key, Some(name));
            let Some(index) = table
                .relationships
                .iter()
                .position(|relationship| relationship.name == *name)
            else {
                if strict {
                    let known: Vec<String> = table
                        .relationships
                        .iter()
                        .map(|relationship| format!("`{}`", relationship.name))
                        .collect();
                    return Err(format!(
                        "{at} in the configuration names no relationship of `{}`; its relationships are {}",
                        table.display_name(),
                        if known.is_empty() {
                            "none".to_owned()
                        } else {
                            known.join(", ")
                        }
                    ));
                }
                continue;
            };
            match change {
                RelationshipChange::Rename(new_name) => {
                    if rust_identifier(new_name).as_deref() != Ok(new_name.as_str()) {
                        return Err(format!(
                            "{at} in the configuration renames the relationship to `{new_name}`, which is not a snake_case Rust identifier"
                        ));
                    }
                    renames.push((table_index, index, new_name.clone(), at));
                }
                RelationshipChange::OptOut => {
                    let relationship = &table.relationships[index];
                    removed.insert((table_index, index), at.clone());
                    let partner = partner(tables, table, relationship);
                    // A join table's foreign key carries its many-to-many
                    // pair: without it, the table no longer joins the two.
                    let foreign_key = match (relationship.kind, partner) {
                        (RelationshipKind::BelongsTo, _) => {
                            Some((table.module_name.as_str(), relationship.column.as_str()))
                        }
                        (RelationshipKind::HasMany | RelationshipKind::HasOne, Some(_)) => Some((
                            relationship.target.as_str(),
                            relationship.target_column.as_str(),
                        )),
                        _ => None,
                    };
                    let through = foreign_key
                        .map(|(join, column)| through_join(tables, join, column))
                        .unwrap_or_default();
                    for side in partner.into_iter().chain(through) {
                        removed.entry(side).or_insert_with(|| at.clone());
                    }
                }
            }
        }
    }
    let mut renamed = Renamed::new();
    for (table_index, index, new_name, at) in renames {
        if let Some(opted_out) = removed.get(&(table_index, index)) {
            return Err(format!(
                "{at} in the configuration renames a relationship that {opted_out} opts out"
            ));
        }
        let table = &mut tables[table_index];
        table.relationships[index].name.clone_from(&new_name);
        renamed
            .entry((table.module_name.clone(), new_name))
            .or_default()
            .push(at);
    }
    for (table_index, table) in tables.iter_mut().enumerate() {
        let mut index = 0;
        table.relationships.retain(|_| {
            let keep = !removed.contains_key(&(table_index, index));
            index += 1;
            keep
        });
    }
    Ok(renamed)
}

/// The other side of `relationship` of `table`: the inverse of a
/// belongs-to, the belongs-to of an inverse, or the many-to-many back
/// through the same join table, as (table index, relationship index).
fn partner(tables: &[Table], table: &Table, relationship: &Relationship) -> Option<(usize, usize)> {
    let target_index = tables
        .iter()
        .position(|other| other.module_name == relationship.target)?;
    let index = tables[target_index]
        .relationships
        .iter()
        .position(|other| {
            let sides = match (relationship.kind, other.kind) {
                (RelationshipKind::ManyToMany, RelationshipKind::ManyToMany) => {
                    match (&relationship.through, &other.through) {
                        (Some(through), Some(back)) => {
                            through.table == back.table
                                && through.column == back.target_column
                                && through.target_column == back.column
                        }
                        _ => false,
                    }
                }
                (RelationshipKind::ManyToMany, _) | (_, RelationshipKind::ManyToMany) => false,
                (kind, other_kind) => {
                    (kind == RelationshipKind::BelongsTo)
                        != (other_kind == RelationshipKind::BelongsTo)
                }
            };
            sides
                && other.target == table.module_name
                && other.column == relationship.target_column
                && other.target_column == relationship.column
        })?;
    Some((target_index, index))
}

/// The many-to-many relationships going through the join table `join` (a
/// module name) on its foreign key `column`, as (table index, relationship
/// index): both sides of the pair the foreign key belongs to.
fn through_join(tables: &[Table], join: &str, column: &str) -> Vec<(usize, usize)> {
    let mut sides = Vec::new();
    for (table_index, table) in tables.iter().enumerate() {
        for (index, relationship) in table.relationships.iter().enumerate() {
            let Some(through) = &relationship.through else {
                continue;
            };
            if through.table == join
                && (through.column == column || through.target_column == column)
            {
                sides.push((table_index, index));
            }
        }
    }
    sides
}

pub(super) fn check_names(table: &Table, renamed: &Renamed) -> Result<(), String> {
    let mut seen: BTreeMap<&str, &Relationship> = BTreeMap::new();
    for relationship in &table.relationships {
        if let Some(earlier) = seen.insert(&relationship.name, relationship) {
            let mut message = format!(
                "relationships of `{}` collide: `{}` names both the {} and the {}",
                table.display_name(),
                relationship.name,
                describe(table, earlier),
                describe(table, relationship)
            );
            match renamed.get(&(table.module_name.clone(), relationship.name.clone())) {
                Some(entries) => message.push_str(&format!(
                    " (the new name {} in the configuration gives); rename it to a free name",
                    entries.join(" and ")
                )),
                None => message.push_str(&format!(
                    "; rename one of them, or opt one out, in {}",
                    location(&table.module_name, None)
                )),
            }
            return Err(message);
        }
    }
    Ok(())
}

fn describe(table: &Table, relationship: &Relationship) -> String {
    match relationship.kind {
        RelationshipKind::BelongsTo => format!(
            "belongs-to `{}` through `{}.{}`",
            relationship.target, table.sql_name, relationship.column
        ),
        RelationshipKind::HasMany | RelationshipKind::HasOne => format!(
            "{} `{}` through `{}.{}`",
            if relationship.kind.is_to_many() {
                "has-many"
            } else {
                "has-one"
            },
            relationship.target,
            relationship.target,
            relationship.target_column
        ),
        RelationshipKind::ManyToMany => format!(
            "many-to-many `{}` through `{}`",
            relationship.target,
            relationship
                .through
                .as_ref()
                .map_or("", |through| through.table.as_str())
        ),
    }
}
