//! Rendering a resource's `relationships.rs`: one loader per relationship,
//! and the resolved relationships the joined reads and filters use.

use std::collections::BTreeMap;

use crate::schema::{Column, Relationship, RelationshipKind, Table};

use super::find_column;
use crate::generator::joins::{self, Join};
use crate::generator::sql::{
    column_clause, first_placeholder, ident, key_order, live_condition, table_ref, tenant_condition,
};
use crate::generator::{
    HEADER, StaticQuery, doc_name, keys, push_doc, push_fn_head, types_import, use_list,
};

/// Renders `table`'s `relationships.rs`, or `None` when it has no
/// relationship. `tables` holds every table generated with it; `serde` adds
/// serde derives to the joined-read structs.
pub fn render(table: &Table, tables: &[Table], serde: bool) -> Option<String> {
    if table.relationships.is_empty() {
        return None;
    }
    let loaders: Vec<Loader<'_>> = table
        .relationships
        .iter()
        .map(|relationship| Loader::new(table, tables, relationship))
        .collect();

    let joins = joins::joins(table, tables);

    let mut output = format!(
        "{HEADER}\n//! Loaders for the relationships of the `{}` table: each loads one\n//! relationship for many rows in a single query.{}\n\n{}use std::collections::HashMap;\n\n",
        table.display_name(),
        if joins.is_empty() {
            ""
        } else {
            "\n//!\n//! Each to-one relationship also has a joined read, `get_with_<relationship>`,\n//! fetching one row together with its related row in a single query."
        },
        joins::sqlx_imports(&joins)
    );
    output.push_str(&imports(table, &loaders, &joins));
    for loader in &loaders {
        loader.render(&mut output);
    }
    for join in &joins {
        join.render(&mut output, serde);
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
    join: Option<JoinTable<'a>>,
}

/// The join table a many-to-many relationship goes through.
pub struct JoinTable<'a> {
    pub table: &'a Table,
    /// The join table's column equal to the relationship's `column`.
    pub column: &'a Column,
    /// The join table's column equal to the relationship's `target_column`.
    pub target_column: &'a Column,
}

/// A relationship of a table, resolved against the tables generated with
/// it: the related table and the columns the two sides join on.
pub struct Resolved<'a> {
    pub target: &'a Table,
    /// The joining column on the relationship's own table.
    pub column: &'a Column,
    /// The joining column on `target`.
    pub target_column: &'a Column,
    /// A many-to-many relationship's join table.
    pub join: Option<JoinTable<'a>>,
}

/// Resolves `table`'s `relationship` against `tables`. Inference only
/// records relationships between tables generated together, over columns
/// they have, so every name resolves.
pub fn resolve<'a>(
    table: &'a Table,
    relationship: &Relationship,
    tables: &'a [Table],
) -> Resolved<'a> {
    let module_table = |module: &str| {
        tables
            .iter()
            .find(|other| other.module_name == module)
            .expect("a relationship's tables are generated with it")
    };
    let column = |table: &'a Table, name: &str| {
        find_column(table, name).expect("a relationship's columns exist")
    };
    let target = module_table(&relationship.target);
    Resolved {
        target,
        column: column(table, &relationship.column),
        target_column: column(target, &relationship.target_column),
        join: relationship.through.as_ref().map(|through| {
            let join = module_table(&through.table);
            JoinTable {
                table: join,
                column: column(join, &through.column),
                target_column: column(join, &through.target_column),
            }
        }),
    }
}

/// The aliases a many-to-many loader's query gives its two tables and the
/// column naming the row each related row was loaded for.
const JOIN_ALIAS: &str = "rok_sqlc_join";
const TARGET_ALIAS: &str = "rok_sqlc_target";
const OWNER_ALIAS: &str = "rok_sqlc_owner";

impl<'a> Loader<'a> {
    fn new(table: &'a Table, tables: &'a [Table], relationship: &'a Relationship) -> Self {
        let Resolved {
            target,
            column,
            target_column,
            join,
        } = resolve(table, relationship, tables);
        Self {
            table,
            relationship,
            target,
            column,
            target_column,
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
    fn many_to_many_query(&self, join: &JoinTable<'_>, output: &mut String) {
        let target = self.target;
        let tenant = keys::tenant_bind(self.tenant_table());
        let placeholder = first_placeholder(self.tenant_table());
        let qualified = |alias: &str, column: &str| format!("{alias}.{}", ident(column));
        let scope = |alias: &str, table: &Table| -> Vec<String> {
            tenant_condition(table, Some(alias))
                .into_iter()
                .chain(live_condition(table, Some(alias)))
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
    fn render_many_to_many(&self, join: &JoinTable<'_>, output: &mut String) {
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
        let placeholder = first_placeholder(target);
        let mut conditions: Vec<String> = tenant_condition(target, None).into_iter().collect();
        conditions.push(format!(
            "{} = ANY(${placeholder})",
            ident(&self.target_column.sql_name)
        ));
        conditions.extend(live_condition(target, None));

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
fn imports(table: &Table, loaders: &[Loader<'_>], joins: &[Join<'_>]) -> String {
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
    for column in joins::param_columns(joins) {
        if let Some(id) = &column.typed_id {
            add(&id.module, &id.name);
        }
        value_types.push(column.value_type().to_owned());
    }
    if let Some(key) = joins::key_struct(table, joins) {
        add(&table.module_name, &key);
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
