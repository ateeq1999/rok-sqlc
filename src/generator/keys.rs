//! Typed ids and key structs.
//!
//! A table with a single-column primary key gets a typed id (`ProductId`), a
//! transparent SQLx type defined in its `models.rs`; the key column, every
//! foreign key column referencing it, and every generated function taking
//! the key use it. A composite primary key gets a key struct
//! (`OrderItemKey`) instead, which lookups and `search_after` cursors take.
//!
//! Typed ids are resolved over the tables generated together: a foreign key
//! to a table outside that set (filtered out, rejected, or in another
//! migration set) keeps the raw type.

use crate::schema::{Column, ForeignKey, Table, TypedId};

use super::{param_name, push_doc};

/// Copies `tables` with every column's typed id filled in.
pub fn resolve(tables: &[Table]) -> Vec<Table> {
    let mut resolved = tables.to_vec();
    for table in &mut resolved {
        let Some(id) = own_typed_id(table) else {
            continue;
        };
        let key = table.primary_key[0].clone();
        if let Some(column) = table.columns.iter_mut().find(|c| c.sql_name == key) {
            column.typed_id = Some(id);
        }
    }
    for table in &mut resolved {
        for foreign_key in &table.foreign_keys {
            let Some((id, key_type)) = referenced_id(tables, foreign_key) else {
                continue;
            };
            let column = table
                .columns
                .iter_mut()
                .find(|column| column.sql_name == foreign_key.column);
            // A key column keeps its own table's id, and a column whose type
            // differs from the referenced key's would not decode as the id.
            if let Some(column) = column
                && column.typed_id.is_none()
                && column.sqlx_type == key_type
            {
                column.typed_id = Some(id);
            }
        }
    }
    resolved
}

/// The typed id of a table with a single-column primary key of a type an id
/// can wrap.
fn own_typed_id(table: &Table) -> Option<TypedId> {
    match table.primary_key_columns().as_slice() {
        [key] if key.supports_typed_id() => Some(TypedId {
            module: table.module_name.clone(),
            name: format!("{}Id", table.struct_name),
        }),
        _ => None,
    }
}

/// The typed id a foreign key's column takes, with the referenced key's raw
/// type: only a reference to the whole primary key of a table in `tables`,
/// matched by schema and name (an unqualified name and `public.` agree).
fn referenced_id(tables: &[Table], foreign_key: &ForeignKey) -> Option<(TypedId, String)> {
    fn normalize(schema: Option<&str>) -> Option<&str> {
        schema.filter(|schema| *schema != "public")
    }
    let target = tables.iter().find(|table| {
        table.sql_name == foreign_key.referenced_table
            && normalize(table.schema.as_deref())
                == normalize(foreign_key.referenced_schema.as_deref())
    })?;
    let id = own_typed_id(target)?;
    match target.primary_key_columns().as_slice() {
        [key] if key.sql_name == foreign_key.referenced_column => Some((id, key.sqlx_type.clone())),
        _ => None,
    }
}

/// The key struct's name for a composite primary key.
fn key_struct_name(table: &Table) -> String {
    format!("{}Key", table.struct_name)
}

/// The type generated functions take a table's key as: the key column's type
/// (its typed id when it has one) or the key struct.
pub fn key_type(table: &Table) -> String {
    match table.primary_key_columns().as_slice() {
        [key] => key.value_type().to_owned(),
        _ => key_struct_name(table),
    }
}

/// The name of the key parameter or binding: the key column's parameter
/// name, or `key` for a key struct.
pub fn key_binding(table: &Table) -> String {
    match table.primary_key_columns().as_slice() {
        [key] => param_name(key),
        _ => "key".to_owned(),
    }
}

/// The key parameter of a generated function, e.g. `id: ProductId`.
pub fn key_param(table: &Table) -> String {
    format!("{}: {}", key_binding(table), key_type(table))
}

/// The bind expressions of the key, one per key column in key order.
pub fn key_binds(table: &Table) -> Vec<String> {
    match table.primary_key_columns().as_slice() {
        [key] => vec![param_name(key)],
        keys => keys
            .iter()
            .map(|key| format!("key.{}", key.rust_name))
            .collect(),
    }
}

/// The names `models.rs` defines for the key: the typed id or key struct;
/// none when the single key column's type cannot be wrapped.
pub fn key_items(table: &Table) -> Vec<String> {
    match table.primary_key_columns().as_slice() {
        [] => Vec::new(),
        [key] => key.typed_id.iter().map(|id| id.name.clone()).collect(),
        _ => vec![key_struct_name(table)],
    }
}

/// `use` paths for the typed ids of `columns` that `table`'s resource files
/// do not define themselves: other resources' ids, and (outside `models.rs`)
/// the resource's own.
pub fn id_imports<'a>(
    table: &Table,
    columns: impl IntoIterator<Item = &'a Column>,
    in_models: bool,
) -> Vec<String> {
    let mut paths: Vec<String> = columns
        .into_iter()
        .filter_map(|column| column.typed_id.as_ref())
        .filter_map(|id| {
            if id.module != table.module_name {
                Some(format!("super::super::{}::models::{}", id.module, id.name))
            } else if in_models {
                None
            } else {
                Some(format!("super::models::{}", id.name))
            }
        })
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

/// Renders the table's typed id or key struct (before the row struct).
pub fn render_key_type(output: &mut String, table: &Table) {
    let keys = table.primary_key_columns();
    match keys.as_slice() {
        [] => {}
        [key] => {
            if let Some(id) = &key.typed_id {
                render_typed_id(output, table, key, &id.name);
            }
        }
        _ => render_key_struct(output, table, &keys),
    }
}

fn render_typed_id(output: &mut String, table: &Table, key: &Column, name: &str) {
    let inner = &key.sqlx_type;
    push_doc(
        output,
        0,
        &[
            &format!(
                "The primary key of the `{}` table, as a distinct type so it",
                table.display_name()
            ),
            "cannot be mixed up with another table's key.",
        ],
    );
    let copy = if key.is_copy() { "Copy, " } else { "" };
    output.push_str(&format!(
        "#[derive(Debug, Clone, {copy}PartialEq, Eq, PartialOrd, Ord, Hash, sqlx::Type)]\n"
    ));
    output.push_str("#[sqlx(transparent)]\n");
    output.push_str(&format!("pub struct {name}(pub {inner});\n\n"));
    output.push_str(&format!(
        "impl From<{inner}> for {name} {{\n    fn from(value: {inner}) -> Self {{\n        Self(value)\n    }}\n}}\n\n"
    ));
    output.push_str(&format!(
        "impl From<{name}> for {inner} {{\n    fn from(id: {name}) -> Self {{\n        id.0\n    }}\n}}\n\n"
    ));
    output.push_str(&format!(
        "impl std::fmt::Display for {name} {{\n    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{\n        std::fmt::Display::fmt(&self.0, formatter)\n    }}\n}}\n\n"
    ));
}

fn render_key_struct(output: &mut String, table: &Table, keys: &[&Column]) {
    push_doc(
        output,
        0,
        &[&format!(
            "The composite primary key of the `{}` table.",
            table.display_name()
        )],
    );
    let derives = if keys.iter().all(|key| key.supports_typed_id()) {
        let copy = if keys.iter().all(|key| key.is_copy()) {
            "Copy, "
        } else {
            ""
        };
        format!("Debug, Clone, {copy}PartialEq, Eq, PartialOrd, Ord, Hash")
    } else {
        "Debug, Clone, PartialEq".to_owned()
    };
    output.push_str(&format!("#[derive({derives})]\n"));
    output.push_str(&format!("pub struct {} {{\n", key_struct_name(table)));
    for key in keys {
        output.push_str(&format!(
            "    pub {}: {},\n",
            key.rust_name,
            key.value_type()
        ));
    }
    output.push_str("}\n\n");
}

/// Renders `impl Row { fn key() }` for a composite key (after the row
/// struct), so a fetched row's key can be passed back, e.g. as a
/// `search_after` cursor.
pub fn render_row_key(output: &mut String, table: &Table) {
    let keys = table.primary_key_columns();
    if keys.len() < 2 {
        return;
    }
    let name = key_struct_name(table);
    output.push_str(&format!("\nimpl {} {{\n", table.struct_name));
    push_doc(output, 4, &["The row's primary key."]);
    output.push_str(&format!("    pub fn key(&self) -> {name} {{\n"));
    output.push_str(&format!("        {name} {{\n"));
    for key in keys {
        let clone = if key.is_copy() { "" } else { ".clone()" };
        output.push_str(&format!(
            "            {}: self.{}{clone},\n",
            key.rust_name, key.rust_name
        ));
    }
    output.push_str("        }\n    }\n}\n");
}
