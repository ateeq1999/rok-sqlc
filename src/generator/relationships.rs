//! Relationships (ADR 0014): inferred from foreign keys over the tables
//! generated together, recorded on the schema model, and rendered as each
//! resource's `relationships.rs` of loaders.
//!
//! A loader takes already-fetched rows and loads one relationship for all of
//! them in a single `= ANY($n)` query, so "with" loading never becomes one
//! query per row. It returns a map keyed by the value the two tables join on
//! (the parent's key): one related row per key for a to-one relationship, a
//! list per key (empty when nothing relates) for a to-many one.

use std::collections::BTreeMap;

use heck::ToSnakeCase;

use crate::parser::naming::rust_identifier;
use crate::schema::{Column, Relationship, RelationshipKind, Table};
use crate::{Conventions, RelationshipChange};

use super::sql::{column_clause, ident, key_order, live_condition, table_ref};
use super::{HEADER, StaticQuery, doc_name, keys, push_doc, push_fn_head, types_import, use_list};

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

/// Infers every relationship among `tables` and records it on both of its
/// tables (`Table::relationships`). Runs after typed ids and conventions are
/// applied: the tenant column's foreign key makes no relationship.
///
/// A foreign key makes a relationship when the referenced table is among
/// `tables` and both columns have the same type, one a map key can hold.
///
/// `changes` (the configuration's `[relationships]`) then renames and opts
/// out relationships; entries for tables or relationships outside this run
/// are skipped, since [`check_changes`] has already checked them against the
/// whole schema. Returns the first name collision between two relationships
/// of a table, or the first invalid change.
pub fn infer(tables: &mut [Table], changes: &Changes) -> Result<(), String> {
    record(tables);
    let renamed = apply_changes(tables, changes, false)?;
    for table in tables.iter() {
        check_names(table, &renamed)?;
        super::search::check_filter_fields(table, tables).map_err(|message| {
            format!(
                "{message}; rename the relationship in {}",
                location(&table.module_name, None)
            )
        })?;
    }
    Ok(())
}

/// The configuration's `[relationships]`: by table, then by inferred
/// relationship name.
pub type Changes = BTreeMap<String, BTreeMap<String, RelationshipChange>>;

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
    let mut resolved = keys::resolve(tables);
    super::conventions::apply(&mut resolved, conventions);
    record(&mut resolved);
    let renamed = apply_changes(&mut resolved, changes, true)?;
    for table in &resolved {
        check_names(table, &renamed)?;
    }
    Ok(())
}

/// Where a `[relationships]` entry lives, as a dotted key path:
/// `relationships.invoices.created_by_user`.
fn location(table: &str, name: Option<&str>) -> String {
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
fn apply_changes(tables: &mut [Table], changes: &Changes, strict: bool) -> Result<Renamed, String> {
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
                    if let Some(partner) = partner(tables, table, relationship) {
                        removed.entry(partner).or_insert(at);
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
/// belongs-to, or the belongs-to of an inverse, as (table index,
/// relationship index).
fn partner(tables: &[Table], table: &Table, relationship: &Relationship) -> Option<(usize, usize)> {
    let belongs_to = relationship.kind == RelationshipKind::BelongsTo;
    let target_index = tables
        .iter()
        .position(|other| other.module_name == relationship.target)?;
    let index = tables[target_index]
        .relationships
        .iter()
        .position(|other| {
            (other.kind == RelationshipKind::BelongsTo) != belongs_to
                && other.target == table.module_name
                && other.column == relationship.target_column
                && other.target_column == relationship.column
        })?;
    Some((target_index, index))
}

/// Infers the relationships of `tables` and records them, replacing any
/// recorded before.
fn record(tables: &mut [Table]) {
    let edges = edges(tables);
    let mut inferred: Vec<Vec<Relationship>> = vec![Vec::new(); tables.len()];
    for edge in &edges {
        inferred[edge.child].push(Relationship {
            name: edge.name.clone(),
            kind: RelationshipKind::BelongsTo,
            target: tables[edge.parent].module_name.clone(),
            column: edge.column.clone(),
            target_column: edge.parent_column.clone(),
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
        });
    }
    for (table, relationships) in tables.iter_mut().zip(inferred) {
        table.relationships = relationships;
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

fn find_column<'a>(table: &'a Table, name: &str) -> Option<&'a Column> {
    table.columns.iter().find(|column| column.sql_name == name)
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

fn check_names(table: &Table, renamed: &Renamed) -> Result<(), String> {
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
    }
}

/// Renders `table`'s `relationships.rs`, or `None` when it has no
/// relationship. `tables` holds every table generated with it.
pub fn render(table: &Table, tables: &[Table]) -> Option<String> {
    if table.relationships.is_empty() {
        return None;
    }
    let loaders: Vec<Loader<'_>> = table
        .relationships
        .iter()
        .map(|relationship| Loader::new(table, tables, relationship))
        .collect();

    let mut output = format!(
        "{HEADER}\n//! Loaders for the relationships of the `{}` table: each loads one\n//! relationship for many rows in a single query.\n\nuse std::collections::HashMap;\n\n",
        table.display_name()
    );
    output.push_str(&imports(table, &loaders));
    for loader in &loaders {
        loader.render(&mut output);
    }
    Some(output)
}

/// One relationship, resolved against both tables.
struct Loader<'a> {
    table: &'a Table,
    relationship: &'a Relationship,
    target: &'a Table,
    /// The joining column on `table`'s rows.
    column: &'a Column,
    /// The joining column on `target`'s rows.
    target_column: &'a Column,
}

impl<'a> Loader<'a> {
    fn new(table: &'a Table, tables: &'a [Table], relationship: &'a Relationship) -> Self {
        let target = tables
            .iter()
            .find(|other| other.module_name == relationship.target)
            .expect("a relationship's target is generated with it");
        Self {
            table,
            relationship,
            target,
            column: find_column(table, &relationship.column).expect("the joining column exists"),
            target_column: find_column(target, &relationship.target_column)
                .expect("the related joining column exists"),
        }
    }

    /// The parent's joining column: its value type keys the loaded map.
    fn key_column(&self) -> &'a Column {
        match self.relationship.kind {
            RelationshipKind::BelongsTo => self.target_column,
            RelationshipKind::HasMany | RelationshipKind::HasOne => self.column,
        }
    }

    fn key_type(&self) -> &'a str {
        self.key_column().value_type()
    }

    /// `value`, an owned value of `column`'s value type, as the key type. The
    /// two differ only when one column carries its own table's typed id (a
    /// primary key that is also a foreign key): the value goes through the
    /// raw type.
    fn convert(&self, column: &Column, value: String) -> String {
        let key = self.key_column();
        if column.value_type() == key.value_type() {
            return value;
        }
        let raw = if column.typed_id.is_some() {
            format!("{}({value})", from_call(&column.sqlx_type))
        } else {
            value
        };
        if key.typed_id.is_some() {
            format!("{}({raw})", from_call(key.value_type()))
        } else {
            raw
        }
    }

    fn function_name(&self) -> String {
        format!("load_{}", self.relationship.name)
    }

    fn render(&self, output: &mut String) {
        let target = self.target;
        let row = &target.struct_name;
        let returns = if self.relationship.kind.is_to_many() {
            format!("Vec<{row}>")
        } else {
            row.clone()
        };
        output.push('\n');
        let doc = self.doc();
        let doc: Vec<&str> = doc.iter().map(String::as_str).collect();
        push_doc(output, 0, &doc);
        let mut params = keys::tenant_params(target);
        params.push(format!("rows: &[{}]", self.table.struct_name));
        push_fn_head(
            output,
            &self.function_name(),
            &params,
            &format!(
                "Result<HashMap<{}, {returns}>, sqlx::Error>",
                self.key_type()
            ),
        );
        if self.relationship.kind.is_to_many() {
            self.render_to_many(output);
        } else {
            self.render_to_one(output);
        }
        output.push_str("}\n");
    }

    fn doc(&self) -> Vec<String> {
        let target = doc_name(&self.target.display_name());
        let column = doc_name(&self.column.sql_name);
        let target_column = doc_name(&self.target_column.sql_name);
        let name = format!("`{}`", self.relationship.name);
        let mut lines = match self.relationship.kind {
            RelationshipKind::BelongsTo => vec![
                format!("Loads the {name} of each of `rows`: the {target} row its {column}"),
                format!("references, keyed by {target_column}, in one query."),
            ],
            RelationshipKind::HasOne => vec![
                format!("Loads the {name} of each of `rows`: the {target} row whose"),
                format!("{target_column} references it, keyed by {column}, in one query."),
            ],
            RelationshipKind::HasMany => vec![
                format!("Loads the {name} of each of `rows`: the {target} rows whose"),
                format!("{target_column} references it, keyed by {column}, in one query."),
                "Every row's key has an entry, empty when no row references it.".to_owned(),
            ],
        };
        if self.target.tenant().is_some() {
            lines.push("Only the given tenant's rows are loaded.".to_owned());
        }
        if self.target.soft_delete_column.is_some() {
            lines.push("Soft-deleted rows are left out.".to_owned());
        }
        lines
    }

    /// The query loading the related rows whose joining column is among
    /// `keys`, bound after the tenant.
    fn query(&self, lead: &str, output: &mut String) {
        let target = self.target;
        let tenant = keys::tenant_bind(target);
        let placeholder = 1 + usize::from(tenant.is_some());
        let mut conditions: Vec<String> = target
            .tenant()
            .map(|tenant| format!("{} = $1", ident(&tenant.sql_name)))
            .into_iter()
            .collect();
        conditions.push(format!(
            "{} = ANY(${placeholder})",
            ident(&self.target_column.sql_name)
        ));
        conditions.extend(live_condition(target));

        let mut sql = column_clause("SELECT", target);
        sql.push(format!("FROM {}", table_ref(target)));
        sql.push(format!("WHERE {}", conditions.join(" AND ")));
        let order = target.primary_key_columns();
        if self.relationship.kind.is_to_many() && !order.is_empty() {
            sql.push(format!("ORDER BY {}", key_order(&order)));
        }
        StaticQuery {
            call: &format!("sqlx::query_as::<_, {}>", target.struct_name),
            sql,
            binds: tenant.into_iter().chain(["keys".to_owned()]).collect(),
            execute: "fetch_all(executor)",
        }
        .push(output, 4, lead, "?;");
    }

    fn render_to_one(&self, output: &mut String) {
        output.push_str("    let mut keys = Vec::new();\n    for row in rows {\n");
        let key = self.owned_key(output, self.column, false);
        output.push_str(&format!("        keys.push({key});\n"));
        output.push_str(
            "    }\n    if keys.is_empty() {\n        return Ok(HashMap::new());\n    }\n",
        );
        self.query("let related = ", output);
        output.push_str("    let mut loaded = HashMap::new();\n    for row in related {\n");
        let key = self.owned_key(output, self.target_column, true);
        output.push_str(&format!("        loaded.insert({key}, row);\n"));
        output.push_str("    }\n    Ok(loaded)\n");
    }

    fn render_to_many(&self, output: &mut String) {
        output.push_str(
            "    let mut loaded = HashMap::new();\n    let mut keys = Vec::new();\n    for row in rows {\n",
        );
        let key = self.owned_key(output, self.column, false);
        output.push_str(&format!(
            "        loaded.insert({key}, Vec::new());\n        keys.push({key});\n"
        ));
        output.push_str("    }\n    if keys.is_empty() {\n        return Ok(loaded);\n    }\n");
        self.query("let related = ", output);
        output.push_str("    for row in related {\n");
        let column = self.target_column;
        let lookup = if column.value_type() != self.key_type() {
            format!("&{}", self.owned_key(output, column, false))
        } else if column.nullable {
            self.push_let_some(output, column, true);
            "key".to_owned()
        } else {
            format!("&row.{}", column.rust_name)
        };
        output.push_str(&format!(
            "        if let Some(list) = loaded.get_mut({lookup}) {{\n            list.push(row);\n        }}\n"
        ));
        output.push_str("    }\n    Ok(loaded)\n");
    }

    /// The owned, key-typed value of `column` on `row`, inside a loop over
    /// rows; a nullable column first skips a row whose value is `NULL`,
    /// binding the value as `key`. With `moving_row`, the expression must
    /// not borrow `row`, which the statement using it moves.
    fn owned_key(&self, output: &mut String, column: &Column, moving_row: bool) -> String {
        let copy = column.is_copy();
        let value = if column.nullable {
            if moving_row && !copy {
                output.push_str(&format!(
                    "        let Some(key) = row.{}.clone() else {{\n            continue;\n        }};\n",
                    column.rust_name
                ));
                "key".to_owned()
            } else {
                self.push_let_some(output, column, !copy);
                if copy { "key" } else { "key.clone()" }.to_owned()
            }
        } else {
            let place = format!("row.{}", column.rust_name);
            if copy {
                place
            } else {
                format!("{place}.clone()")
            }
        };
        self.convert(column, value)
    }

    /// `let Some(key) = row.<column> else { continue; };`, borrowing the
    /// value when `borrow` is set.
    fn push_let_some(&self, output: &mut String, column: &Column, borrow: bool) {
        let borrow = if borrow { "&" } else { "" };
        output.push_str(&format!(
            "        let Some(key) = {borrow}row.{} else {{\n            continue;\n        }};\n",
            column.rust_name
        ));
    }
}

/// `Type::from`, written `<Type>::from` when the type has generic arguments.
fn from_call(rust_type: &str) -> String {
    if rust_type.contains('<') {
        format!("<{rust_type}>::from")
    } else {
        format!("{rust_type}::from")
    }
}

/// The `use` lines for the row types, typed ids, and enums the loaders name,
/// one line per module, sorted the way rustfmt sorts them.
fn imports(table: &Table, loaders: &[Loader<'_>]) -> String {
    let mut by_module: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut add = |module: &str, name: &str| {
        let path = if module == table.module_name {
            "super::models".to_owned()
        } else {
            format!("super::super::{module}::models")
        };
        by_module.entry(path).or_default().push(name.to_owned());
    };
    add(&table.module_name, &table.struct_name);
    let mut value_types = Vec::new();
    for loader in loaders {
        add(&loader.target.module_name, &loader.target.struct_name);
        let tenant = loader.target.tenant();
        for column in [Some(loader.key_column()), tenant].into_iter().flatten() {
            if let Some(id) = &column.typed_id {
                add(&id.module, &id.name);
            }
            value_types.push(column.value_type().to_owned());
        }
    }
    let mut lines: Vec<(Vec<(bool, String)>, String)> = by_module
        .into_iter()
        .map(|(path, mut names)| {
            names.sort();
            names.dedup();
            let key = path
                .split("::")
                .map(|segment| (segment != "super", segment.to_owned()))
                .collect();
            (key, use_list(&path, &names))
        })
        .collect();
    let types = types_import("super::super", value_types.iter().map(String::as_str));
    if !types.is_empty() {
        lines.push((
            vec![
                (false, "super".to_owned()),
                (false, "super".to_owned()),
                (true, "types".to_owned()),
            ],
            types,
        ));
    }
    lines.sort();
    lines.into_iter().map(|(_, line)| line).collect()
}
