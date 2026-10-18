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
//!
//! An enum key's typed id wraps the generated enum (`types::X`). Enums are
//! neither `Ord` nor `Display`, so neither is that id.

use crate::IdGeneration;
use crate::schema::{BaseType, Column, ForeignKey, Table, TypedId};

use super::{DeriveOptions, param_name, param_type, push_derives, push_doc, serde_derives};

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

/// Fails when a resource's files would import two typed ids of one name
/// (from two tables whose struct names agree, such as `user` and `users`),
/// or an imported typed id would take the name of an item the resource
/// defines itself: its row struct, typed id or key struct, create input, or
/// update patch. Runs on `tables` as [`resolve`] and the conventions left
/// them.
pub fn check_names(tables: &[Table]) -> Result<(), String> {
    const HINT: &str = "rename a module under `[modules]`";
    let table_of = |module: &str| {
        tables
            .iter()
            .find(|table| table.module_name == module)
            .map_or_else(|| module.to_owned(), Table::display_name)
    };
    for table in tables {
        let mut imported: Vec<&TypedId> = table
            .columns
            .iter()
            .filter_map(|column| column.typed_id.as_ref())
            .filter(|id| id.module != table.module_name)
            .collect();
        imported.sort_by(|a, b| (&a.name, &a.module).cmp(&(&b.name, &b.module)));
        imported.dedup();
        for pair in imported.windows(2) {
            if pair[0].name == pair[1].name {
                return Err(format!(
                    "`{}` would import two typed ids named `{}`, from `{}` and `{}`; {HINT}",
                    table.display_name(),
                    pair[0].name,
                    table_of(&pair[0].module),
                    table_of(&pair[1].module)
                ));
            }
        }
        let own_kind = if table.key_columns().len() > 1 {
            "key struct"
        } else {
            "typed id"
        };
        let mut defined: Vec<(&str, String)> = vec![("row struct", table.struct_name.clone())];
        defined.extend(key_items(table).into_iter().map(|name| (own_kind, name)));
        if !table.insert_columns().is_empty() {
            defined.push(("create input", format!("New{}", table.struct_name)));
        }
        if !table.primary_key.is_empty() && !table.update_columns().is_empty() {
            defined.push(("update patch", format!("Update{}", table.struct_name)));
        }
        for id in imported {
            if let Some((kind, name)) = defined.iter().find(|(_, name)| *name == id.name) {
                return Err(format!(
                    "the typed id `{}` imported from `{}` collides with the {kind} `{name}` that `{}` generates; {HINT}",
                    id.name,
                    table_of(&id.module),
                    table.display_name()
                ));
            }
        }
    }
    Ok(())
}

/// Whether a typed id can wrap the key column's type: one that is `Eq`,
/// `Ord`, `Hash`, and `Display`, or a generated enum.
fn wraps_in_typed_id(key: &Column) -> bool {
    key.supports_typed_id() || key.base_type() == BaseType::Enum
}

/// The typed id of a table with a single-column primary key of a type an id
/// can wrap.
fn own_typed_id(table: &Table) -> Option<TypedId> {
    match table.primary_key_columns().as_slice() {
        [key] if wraps_in_typed_id(key) => Some(TypedId {
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
    let target = referenced_table(tables, foreign_key)?;
    let id = own_typed_id(target)?;
    match target.primary_key_columns().as_slice() {
        [key] if key.sql_name == foreign_key.referenced_column => Some((id, key.sqlx_type.clone())),
        _ => None,
    }
}

/// The table among `tables` a foreign key references, matched exactly by
/// schema and name (an unqualified name and `public.` agree).
pub fn referenced_table<'a>(tables: &'a [Table], foreign_key: &ForeignKey) -> Option<&'a Table> {
    fn normalize(schema: Option<&str>) -> Option<&str> {
        schema.filter(|schema| *schema != "public")
    }
    tables.iter().find(|table| {
        table.sql_name == foreign_key.referenced_table
            && normalize(table.schema.as_deref())
                == normalize(foreign_key.referenced_schema.as_deref())
    })
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
pub fn render_key_type(output: &mut String, table: &Table, derives: DeriveOptions) {
    let DeriveOptions {
        serde, validate, ..
    } = derives;
    let keys = table.key_columns();
    match keys.as_slice() {
        [] => {}
        [key] => {
            if let Some(id) = key
                .typed_id
                .as_ref()
                .filter(|id| id.module == table.module_name)
            {
                render_typed_id(output, table, key, &id.name, derives);
                if validate {
                    match key.base_type() {
                        BaseType::Text => super::validate::push_measured_id(output, &id.name),
                        BaseType::Integer => {
                            super::validate::push_ranged_id(output, &id.name, &key.sqlx_type);
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => render_key_struct(output, table, &keys, serde),
    }
}

/// With `serde`, a typed id serializes as its inner value. An enum's id is
/// `Copy` but neither `Ord` nor `Display`, like the enum.
fn render_typed_id(
    output: &mut String,
    table: &Table,
    key: &Column,
    name: &str,
    derives: DeriveOptions,
) {
    let serde = derives.serde;
    let inner = &key.sqlx_type;
    let is_enum = key.base_type() == BaseType::Enum;
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
    let copy = if key.is_copy() || is_enum {
        "Copy, "
    } else {
        ""
    };
    let ord = if is_enum { "" } else { "PartialOrd, Ord, " };
    output.push_str(&format!(
        "#[derive(Debug, Clone, {copy}PartialEq, Eq, {ord}Hash, sqlx::Type)]\n"
    ));
    push_derives(output, &derives.value_derives());
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
    if is_enum {
        return;
    }
    output.push_str(&format!(
        "impl std::fmt::Display for {name} {{\n    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{\n        std::fmt::Display::fmt(&self.0, formatter)\n    }}\n}}\n\n"
    ));
    if let (Some(ids), Some(expr)) = (key.id_generation, key.new_id_expr()) {
        let what = match ids {
            IdGeneration::UuidV4 => "a random UUID (version 4)",
            IdGeneration::UuidV7 => "a UUID version 7, ordered by creation time",
            IdGeneration::Ulid => "a ULID, ordered by creation time",
            IdGeneration::Cuid2 => "a CUID2, random and collision-resistant",
        };
        output.push_str(&format!("impl {name} {{\n"));
        push_doc(
            output,
            4,
            &[&format!(
                "A new id: {what}. `create` uses it when the input leaves the id out."
            )],
        );
        output.push_str(&format!(
            "    #[must_use]\n    pub fn generate() -> Self {{\n        Self({expr})\n    }}\n}}\n\n"
        ));
    }
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
    push_derives(output, serde_derives(serde));
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
