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

use super::{param_name, param_type, push_doc, push_serde_derive};

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
/// (its typed id when it has one) or the key struct. The tenant column is
/// not part of it (see [`Table::key_columns`]).
pub fn key_type(table: &Table) -> String {
    match table.key_columns().as_slice() {
        [] => "()".to_owned(),
        [key] => key.value_type().to_owned(),
        _ => key_struct_name(table),
    }
}

/// The name of the key parameter or binding: the key column's parameter
/// name, or `key` for a key struct.
pub fn key_binding(table: &Table) -> String {
    match table.key_columns().as_slice() {
        [] => "()".to_owned(),
        [key] => param_name(key),
        _ => "key".to_owned(),
    }
}

/// The key parameters of a generated function: the tenant parameter, when
/// the table has a tenant column, then the key, e.g. `id: ProductId`. A
/// primary key that is only the tenant column takes no key parameter.
pub fn key_params(table: &Table) -> Vec<String> {
    let mut params = tenant_params(table);
    if !table.key_columns().is_empty() {
        params.push(key_param(table));
    }
    params
}

/// The arguments passing a function's own key parameters on to another
/// generated function taking them.
pub fn key_args(table: &Table) -> Vec<String> {
    let mut args: Vec<String> = table.tenant().map(param_name).into_iter().collect();
    if !table.key_columns().is_empty() {
        args.push(key_binding(table));
    }
    args
}

/// The tenant parameter, e.g. `organization_id: uuid::Uuid`, which every
/// generated function on a tenant table takes right after the executor; no
/// parameter without a tenant column.
pub fn tenant_params(table: &Table) -> Vec<String> {
    table
        .tenant()
        .map(|tenant| format!("{}: {}", param_name(tenant), param_type(tenant)))
        .into_iter()
        .collect()
}

/// `params` preceded by the tenant parameter, when the table has one.
pub fn scoped_params(table: &Table, params: &[String]) -> Vec<String> {
    let mut scoped = tenant_params(table);
    scoped.extend(params.iter().cloned());
    scoped
}

/// The tenant parameter's bind expression, when the table has a tenant
/// column. Static SQL binds it as `$1`.
pub fn tenant_bind(table: &Table) -> Option<String> {
    table.tenant().map(param_name)
}

/// The type of the key parameter: [`key_type`], borrowed when it is not
/// `Copy` (a text key, or a key struct with a text column).
pub fn key_param_type(table: &Table) -> String {
    match table.key_columns().as_slice() {
        [key] => param_type(key),
        keys if keys.iter().all(|key| key.is_copy()) => key_struct_name(table),
        _ => format!("&{}", key_struct_name(table)),
    }
}

/// The key parameter of a generated function, e.g. `id: ProductId`.
pub fn key_param(table: &Table) -> String {
    format!("{}: {}", key_binding(table), key_param_type(table))
}

/// The bind expressions of the key, one per key column in key order. A
/// borrowed key struct's fields that are not `Copy` are bound by reference.
pub fn key_binds(table: &Table) -> Vec<String> {
    match table.key_columns().as_slice() {
        [key] => vec![param_name(key)],
        keys => keys
            .iter()
            .map(|key| {
                let borrow = if key.is_copy() { "" } else { "&" };
                format!("{borrow}key.{}", key.rust_name)
            })
            .collect(),
    }
}

/// The bind expressions of the tenant (when the table has one) and the key,
/// in placeholder order.
pub fn scoped_key_binds(table: &Table) -> Vec<String> {
    let mut binds: Vec<String> = tenant_bind(table).into_iter().collect();
    binds.extend(key_binds(table));
    binds
}

/// The names `models.rs` defines for the key: the typed id or key struct;
/// none when the single key column's type cannot be wrapped.
pub fn key_items(table: &Table) -> Vec<String> {
    match table.key_columns().as_slice() {
        [] => Vec::new(),
        [key] => key
            .typed_id
            .iter()
            .filter(|id| id.module == table.module_name)
            .map(|id| id.name.clone())
            .collect(),
        _ => vec![key_struct_name(table)],
    }
}

/// The columns whose own types the key parameters name: the tenant column
/// and a single key column (a key struct names its fields' types itself).
pub fn param_columns(table: &Table) -> Vec<&Column> {
    let keys = table.key_columns();
    let single = if keys.len() == 1 { keys } else { Vec::new() };
    table.tenant().into_iter().chain(single).collect()
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
pub fn render_key_type(output: &mut String, table: &Table, serde: bool) {
    let keys = table.key_columns();
    match keys.as_slice() {
        [] => {}
        [key] => {
            if let Some(id) = key
                .typed_id
                .as_ref()
                .filter(|id| id.module == table.module_name)
            {
                render_typed_id(output, table, key, &id.name, serde);
            }
        }
        _ => render_key_struct(output, table, &keys, serde),
    }
}

/// With `serde`, a typed id serializes as its inner value.
fn render_typed_id(output: &mut String, table: &Table, key: &Column, name: &str, serde: bool) {
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
    push_serde_derive(output, serde);
    output.push_str("#[sqlx(transparent)]\n");
    if serde {
        output.push_str("#[serde(transparent)]\n");
    }
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

fn render_key_struct(output: &mut String, table: &Table, keys: &[&Column], serde: bool) {
    push_doc(
        output,
        0,
        &[&format!(
            "The composite primary key of the `{}` table{}.",
            table.display_name(),
            if table.key_columns().len() < table.primary_key.len() {
                ", without the tenant column"
            } else {
                ""
            }
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
    push_serde_derive(output, serde);
    output.push_str(&format!("pub struct {} {{\n", key_struct_name(table)));
    for key in keys {
        push_doc(
            output,
            4,
            &[&format!("The {} column.", super::doc_name(&key.sql_name))],
        );
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
    let keys = table.key_columns();
    if keys.len() < 2 {
        return;
    }
    let name = key_struct_name(table);
    output.push_str(&format!("\nimpl {} {{\n", table.struct_name));
    push_doc(output, 4, &["The row's primary key."]);
    output.push_str(&format!(
        "    #[must_use]\n    pub fn key(&self) -> {name} {{\n"
    ));
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
