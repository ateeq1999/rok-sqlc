//! `create`, `update`, and `delete`; `attach`, `detach`, and `sync` on a join table.
//!
//! Statements whose shape is fixed at generation time are emitted as static
//! SQL. Statements whose shape depends on which optional fields are present
//! (an insert with defaulted columns, a partial update) are built at runtime
//! with `sqlx::QueryBuilder`, whose `push_bind` allocates each placeholder as
//! it binds the value, so placeholder numbers and bind order cannot drift
//! apart. Only identifiers from the schema reach the SQL text.

mod create;
mod delete;
mod join;
#[cfg(test)]
mod tests;
mod update;

use crate::schema::{Column, Table};

use self::create::{render_dynamic_create, render_static_create};
use self::delete::render_delete;
use self::join::render_join_writes;
use self::update::{render_update, render_versioned_update};
use super::relationships::join_pair;
use super::sql::column_clause;
use super::{HEADER, types_import, use_list};

pub use self::update::{has_versioned_update, version_column};

pub fn render(table: &Table) -> String {
    let insert_columns = table.insert_columns();
    let update_columns = table.update_columns();
    let keys = table.primary_key_columns();
    let has_update = !keys.is_empty() && !update_columns.is_empty();

    let mut imports = vec![table.struct_name.clone()];
    if !insert_columns.is_empty() {
        imports.push(format!("New{}", table.struct_name));
    }
    if has_update {
        imports.push(format!("Update{}", table.struct_name));
    }
    imports.extend(super::keys::key_items(table));
    imports.sort();
    // Key parameters use the key's type: an enum key column's type, never a
    // key struct's fields.
    let mut key_types: Vec<String> = table
        .tenant()
        .map(|tenant| tenant.value_type().to_owned())
        .into_iter()
        .collect();
    if !table.key_columns().is_empty() {
        key_types.push(super::keys::key_type(table));
    }
    let join = join_pair(table);
    let pair_columns: Vec<&Column> = join.iter().flat_map(|pair| [pair.near, pair.far]).collect();
    key_types.extend(
        pair_columns
            .iter()
            .map(|column| column.value_type().to_owned()),
    );
    let types = types_import("super::super", key_types.iter().map(String::as_str));
    let key_columns = table.key_columns();
    let ids = super::use_paths(&super::keys::id_imports(
        table,
        super::keys::param_columns(table)
            .into_iter()
            .chain(pair_columns.iter().copied()),
        true,
    ));
    let outcome = if has_versioned_update(table) {
        "use super::super::UpdateOutcome;\n"
    } else {
        ""
    };
    let mut output = format!("{HEADER}\n{outcome}{types}{ids}{}", use_models(&imports));

    if insert_columns.is_empty() {
        output.push_str(
            "\n// No insertable columns detected; create is not generated for this table.\n",
        );
    } else if insert_columns
        .iter()
        .any(|column| column.is_omittable_on_insert())
    {
        render_dynamic_create(&mut output, table, &insert_columns);
    } else {
        render_static_create(&mut output, table, &insert_columns);
    }

    if keys.is_empty() {
        output.push_str(
            "\n// No primary key detected; update and delete are not generated for this table.\n",
        );
    } else {
        render_key_writes(&mut output, table, &keys, &key_columns, has_update);
    }
    if let Some(pair) = &join {
        render_join_writes(&mut output, table, pair);
    }
    output
}

/// `update` and `delete` (with `restore` and `delete_permanently` on a
/// soft-delete table), for a table with a primary key.
fn render_key_writes(
    output: &mut String,
    table: &Table,
    keys: &[&Column],
    key_columns: &[&Column],
    has_update: bool,
) {
    let update_columns = table.update_columns();
    if has_update {
        match version_column(table) {
            Some(version) => {
                render_versioned_update(output, table, version, keys, &update_columns);
            }
            None => render_update(output, table, &update_columns),
        }
    } else {
        output.push_str(
            "\n// No updatable columns detected; update is not generated for this table.\n",
        );
    }
    render_delete(output, table, key_columns);
}

/// `use super::models::...;` for sorted `names`, wrapped like rustfmt wraps a
/// use list that does not fit on one line.
fn use_models(names: &[String]) -> String {
    use_list("super::models", names)
}

fn returning_clause(table: &Table) -> Vec<String> {
    column_clause("RETURNING", table)
}

fn push_returning(output: &mut String, table: &Table) {
    output.push_str("    query.push(\n");
    output.push_str(&super::sql::raw_block(&returning_clause(table), 8));
    output.push_str(",\n    );\n");
}
