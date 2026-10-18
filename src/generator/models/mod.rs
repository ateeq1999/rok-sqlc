//! A table's `models.rs`: its row, its key type, its create input and its
//! update patch ([`inputs`]), and `db/nullable.rs` ([`nullable`]).

mod inputs;
mod nullable;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod validate_tests;

use crate::schema::{Column, Table, mentions_generated_type};

use self::inputs::{render_insert_input, render_update_patch};
use super::{
    DeriveOptions, HEADER, doc_name, keys, push_derives, push_doc, serde_derives, use_paths,
};

pub use self::nullable::render_nullable;

/// The doc line of a row, input, or patch field: the column it holds.
fn column_doc(column: &Column) -> String {
    format!("The {} column.", doc_name(&column.sql_name))
}

pub fn render(table: &Table, derives: DeriveOptions) -> String {
    let mut output = format!("{HEADER}\n");
    let mut imports = keys::id_imports(table, &table.columns, true);
    // A column typed by another module's id (one wrapping an enum) does not
    // name the enum here; this module's own id does, in its definition.
    if table.columns.iter().any(|column| {
        mentions_generated_type(&column.rust_type)
            && column
                .typed_id
                .as_ref()
                .is_none_or(|id| id.module == table.module_name)
    }) {
        imports.push("super::super::types".to_owned());
    }
    if !imports.is_empty() {
        output.push_str(&use_paths(&imports));
        output.push('\n');
    }
    keys::render_key_type(&mut output, table, derives);
    render_row(&mut output, table, derives.serde);
    keys::render_row_key(&mut output, table);

    let insert_columns = table.insert_columns();
    if !insert_columns.is_empty() {
        render_insert_input(&mut output, table, &insert_columns, derives);
    }

    let mut update_columns = table.update_columns();
    if !table.primary_key.is_empty() && !update_columns.is_empty() {
        render_update_patch(&mut output, table, &update_columns, derives);
    } else {
        update_columns.clear();
    }

    if derives.validate {
        let fields = insert_columns.into_iter().chain(update_columns);
        super::validate::push_decimal_checks(&mut output, fields);
    }
    output
}

fn render_row(output: &mut String, table: &Table, serde: bool) {
    let doc = format!("A row of the `{}` table.", table.display_name());
    push_doc(output, 0, &[&doc]);
    output.push_str("#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]\n");
    push_derives(output, serde_derives(serde));
    output.push_str(&format!("pub struct {} {{\n", table.struct_name));
    for column in &table.columns {
        push_doc(output, 4, &[&column_doc(column)]);
        if column.rust_name != column.sql_name {
            output.push_str(&format!(
                "    #[sqlx(rename = {})]\n",
                super::sql::str_literal(&column.sql_name)
            ));
        }
        output.push_str(&format!(
            "    pub {}: {},\n",
            column.rust_name,
            column.field_type()
        ));
    }
    output.push_str("}\n");
}
