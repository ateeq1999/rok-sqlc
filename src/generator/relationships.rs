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
use crate::schema::{Column, Relationship, RelationshipKind, Table, Through};

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
/// Returns the first name collision between two relationships of a table.
pub fn infer(tables: &mut [Table]) -> Result<(), String> {
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
    for table in tables.iter() {
        check_names(table)?;
    }
    Ok(())
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
fn many_to_many(tables: &[Table]) -> Vec<Vec<Relationship>> {
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

fn check_names(table: &Table) -> Result<(), String> {
    let mut seen: BTreeMap<&str, &Relationship> = BTreeMap::new();
    for relationship in &table.relationships {
        if let Some(earlier) = seen.insert(&relationship.name, relationship) {
            return Err(format!(
                "relationships of `{}` collide: `{}` names both the {} and the {}",
                table.display_name(),
                relationship.name,
                describe(table, earlier),
                describe(table, relationship)
            ));
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
    /// A many-to-many relationship's join table.
    join: Option<Join<'a>>,
}

/// The join table a many-to-many loader goes through.
struct Join<'a> {
    table: &'a Table,
    /// The join table's column equal to the loader's `column`.
    column: &'a Column,
    /// The join table's column equal to the loader's `target_column`.
    target_column: &'a Column,
}

/// The aliases a many-to-many loader's query gives its two tables and the
/// column naming the row each related row was loaded for.
const JOIN_ALIAS: &str = "rok_sqlc_join";
const TARGET_ALIAS: &str = "rok_sqlc_target";
const OWNER_ALIAS: &str = "rok_sqlc_owner";

fn module_table<'a>(tables: &'a [Table], module: &str) -> &'a Table {
    tables
        .iter()
        .find(|other| other.module_name == module)
        .expect("a relationship's tables are generated with it")
}

impl<'a> Loader<'a> {
    fn new(table: &'a Table, tables: &'a [Table], relationship: &'a Relationship) -> Self {
        let target = module_table(tables, &relationship.target);
        let join = relationship.through.as_ref().map(|through| {
            let join = module_table(tables, &through.table);
            Join {
                table: join,
                column: find_column(join, &through.column).expect("the join column exists"),
                target_column: find_column(join, &through.target_column)
                    .expect("the join column exists"),
            }
        });
        Self {
            table,
            relationship,
            target,
            column: find_column(table, &relationship.column).expect("the joining column exists"),
            target_column: find_column(target, &relationship.target_column)
                .expect("the related joining column exists"),
            join,
        }
    }

    /// The parent's joining column: its value type keys the loaded map.
    fn key_column(&self) -> &'a Column {
        match self.relationship.kind {
            RelationshipKind::BelongsTo => self.target_column,
            RelationshipKind::HasMany | RelationshipKind::HasOne | RelationshipKind::ManyToMany => {
                self.column
            }
        }
    }

    /// The table whose tenant column the loader takes the tenant for: the
    /// related table, or a many-to-many's join table when only it has one.
    fn tenant_table(&self) -> &'a Table {
        match &self.join {
            Some(join) if self.target.tenant().is_none() && join.table.tenant().is_some() => {
                join.table
            }
            _ => self.target,
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
        let mut params = keys::tenant_params(self.tenant_table());
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
        if let Some(join) = &self.join {
            self.render_many_to_many(join, output);
        } else if self.relationship.kind.is_to_many() {
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
            RelationshipKind::ManyToMany => {
                let through = self
                    .join
                    .as_ref()
                    .map_or(String::new(), |join| doc_name(&join.table.display_name()));
                vec![
                    format!("Loads the {name} of each of `rows`: the {target} rows a"),
                    format!("{through} row pairs it with, keyed by {column}, in one query."),
                    "Every row's key has an entry, empty when no row is paired with it.".to_owned(),
                ]
            }
        };
        let tables: Vec<&Table> = std::iter::once(self.target)
            .chain(self.join.as_ref().map(|join| join.table))
            .collect();
        if tables.iter().any(|table| table.tenant().is_some()) {
            lines.push("Only the given tenant's rows are loaded.".to_owned());
        }
        if tables
            .iter()
            .any(|table| table.soft_delete_column.is_some())
        {
            lines.push("Soft-deleted rows are left out.".to_owned());
        }
        lines
    }

    /// The query loading the related rows paired, through the join table,
    /// with a row whose joining column is among `keys`, bound after the
    /// tenant. Each row also carries that key as `rok_sqlc_owner`. The
    /// related table's tenant and soft-delete conditions go in the `JOIN`'s
    /// `ON` clause, the join table's in `WHERE`.
    fn many_to_many_query(&self, join: &Join<'_>, output: &mut String) {
        let target = self.target;
        let tenant = keys::tenant_bind(self.tenant_table());
        let placeholder = 1 + usize::from(tenant.is_some());
        let qualified = |alias: &str, column: &str| format!("{alias}.{}", ident(column));
        let scope = |alias: &str, table: &Table| -> Vec<String> {
            table
                .tenant()
                .map(|tenant| format!("{} = $1", qualified(alias, &tenant.sql_name)))
                .into_iter()
                .chain(
                    table
                        .soft_delete_column
                        .as_deref()
                        .map(|column| format!("{} IS NULL", qualified(alias, column))),
                )
                .collect()
        };

        let mut on = vec![format!(
            "{} = {}",
            qualified(TARGET_ALIAS, &self.target_column.sql_name),
            qualified(JOIN_ALIAS, &join.target_column.sql_name)
        )];
        on.extend(scope(TARGET_ALIAS, target));
        let mut conditions = scope(JOIN_ALIAS, join.table);
        let owner = qualified(JOIN_ALIAS, &join.column.sql_name);
        conditions.insert(
            usize::from(join.table.tenant().is_some()),
            format!("{owner} = ANY(${placeholder})"),
        );

        let mut sql = vec![
            "SELECT".to_owned(),
            format!("    {owner} AS {OWNER_ALIAS},"),
        ];
        let columns = &target.columns;
        for (index, column) in columns.iter().enumerate() {
            let separator = if index + 1 < columns.len() { "," } else { "" };
            sql.push(format!(
                "    {}{separator}",
                qualified(TARGET_ALIAS, &column.sql_name)
            ));
        }
        sql.push(format!("FROM {} AS {JOIN_ALIAS}", table_ref(join.table)));
        sql.push(format!("JOIN {} AS {TARGET_ALIAS}", table_ref(target)));
        sql.push(format!("    ON {}", on.join(" AND ")));
        sql.push(format!("WHERE {}", conditions.join(" AND ")));
        let order: Vec<String> = target
            .primary_key_columns()
            .iter()
            .map(|column| qualified(TARGET_ALIAS, &column.sql_name))
            .collect();
        if !order.is_empty() {
            sql.push(format!("ORDER BY {}", order.join(", ")));
        }
        StaticQuery {
            call: "sqlx::query",
            sql,
            binds: tenant.into_iter().chain(["keys".to_owned()]).collect(),
            execute: "fetch_all(executor)",
        }
        .push(output, 4, "let related = ", "?;");
    }

    /// A many-to-many loader's body: like a has-many's, but each loaded row
    /// says which key it was loaded for (`rok_sqlc_owner`), since one related
    /// row can be paired with several of `rows`.
    fn render_many_to_many(&self, join: &Join<'_>, output: &mut String) {
        self.push_key_map(output);
        self.many_to_many_query(join, output);
        output.push_str("    for row in related {\n");
        output.push_str(&format!(
            "        let owner: {} = sqlx::Row::try_get(&row, \"{OWNER_ALIAS}\")?;\n",
            join.column.value_type()
        ));
        let owner = self.convert(join.column, "owner".to_owned());
        output.push_str(&format!(
            "        if let Some(list) = loaded.get_mut(&{owner}) {{\n            list.push(sqlx::FromRow::from_row(&row)?);\n        }}\n"
        ));
        output.push_str("    }\n    Ok(loaded)\n");
    }

    /// `loaded`, with an empty list for every row's key, and `keys`; returns
    /// the empty map when there are no keys.
    fn push_key_map(&self, output: &mut String) {
        output.push_str(
            "    let mut loaded = HashMap::new();\n    let mut keys = Vec::new();\n    for row in rows {\n",
        );
        let key = self.owned_key(output, self.column, false);
        output.push_str(&format!(
            "        loaded.insert({key}, Vec::new());\n        keys.push({key});\n"
        ));
        output.push_str("    }\n    if keys.is_empty() {\n        return Ok(loaded);\n    }\n");
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
        self.push_key_map(output);
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
        let tenant = loader.tenant_table().tenant();
        let owner = loader.join.as_ref().map(|join| join.column);
        for column in [Some(loader.key_column()), tenant, owner]
            .into_iter()
            .flatten()
        {
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
