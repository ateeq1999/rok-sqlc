//! A table's create input (`NewXxx`) and update patch (`UpdateXxx`) in
//! its `models.rs`.

use super::super::{
    CHAIN_WIDTH, DeriveOptions, doc_name, insert_field_type, push_and_chain, push_chain,
    push_derives, push_doc, update_field_type,
};
use super::column_doc;
use super::nullable::NULLABLE_DESERIALIZE;
use crate::schema::{Column, Table};

/// The `#[serde(...)]` attribute of an `Option` field of an input or patch
/// (ADR 0013): serializing skips `None`, and an `Option<Option<T>>`
/// (`double`) reads `null` as `Some(None)` through the generated
/// `db::nullable` helper. `default` makes an absent field `None` in a struct
/// without `#[serde(default)]`.
fn push_option_serde(output: &mut String, double: bool, default: bool) {
    if !double {
        output.push_str("    #[serde(skip_serializing_if = \"Option::is_none\")]\n");
        return;
    }
    output.push_str("    #[serde(\n");
    if default {
        output.push_str("        default,\n");
    }
    output.push_str(&format!(
        "        deserialize_with = \"{NULLABLE_DESERIALIZE}\",\n        skip_serializing_if = \"Option::is_none\"\n    )]\n"
    ));
}

pub(super) fn render_insert_input(
    output: &mut String,
    table: &Table,
    columns: &[&Column],
    derives: DeriveOptions,
) {
    let DeriveOptions {
        serde, validate, ..
    } = derives;
    output.push('\n');
    let has_defaults = columns.iter().any(|column| column.is_omittable_on_insert());
    let mut doc = vec!["Input for `create`: every column the caller may supply."];
    if has_defaults {
        doc.extend([
            "",
            "`Option` fields are columns with a database default: `None` leaves the",
            "column out of the `INSERT` so the default applies.",
        ]);
    }
    push_doc(output, 0, &doc);

    // Every field defaulting to `None` is a meaningful insert: it omits every
    // defaulted column and writes NULL to every nullable one.
    let all_optional = columns
        .iter()
        .all(|column| column.is_optional_in_input() || column.nullable);
    if all_optional {
        output.push_str("#[derive(Debug, Clone, Default, PartialEq)]\n");
    } else {
        output.push_str("#[derive(Debug, Clone, PartialEq)]\n");
    }
    push_derives(output, &derives.input_derives());
    output.push_str(&format!("pub struct New{} {{\n", table.struct_name));
    for column in columns {
        let doc = column_doc(column);
        if column.is_omittable_on_insert() && column.nullable {
            push_doc(
                output,
                4,
                &[
                    &doc,
                    "`None` applies the column default; `Some(None)` inserts `NULL`.",
                ],
            );
        } else if column.is_omittable_on_insert() {
            push_doc(output, 4, &[&doc, "`None` applies the column default."]);
        } else if let (Some(_), Some(id)) = (column.id_generation, &column.typed_id) {
            push_doc(
                output,
                4,
                &[
                    &doc,
                    &format!("`None` inserts a new id from `{}::generate`.", id.name),
                ],
            );
        } else if column.id_generation.is_some() {
            push_doc(output, 4, &[&doc, "`None` inserts a new id."]);
        } else {
            push_doc(output, 4, &[&doc]);
        }
        // Plain serde reads both an absent field and `null` as `None`, which
        // would make `Some(None)` unreachable from JSON.
        if serde && column.is_omittable_on_insert() && column.nullable {
            push_option_serde(output, true, true);
        }
        if validate {
            super::super::validate::push_field_rule(output, column);
        }
        output.push_str(&format!(
            "    pub {}: {},\n",
            column.rust_name,
            insert_field_type(column)
        ));
    }
    output.push_str("}\n");
}

pub(super) fn render_update_patch(
    output: &mut String,
    table: &Table,
    columns: &[&Column],
    derives: DeriveOptions,
) {
    let DeriveOptions {
        serde, validate, ..
    } = derives;
    output.push('\n');
    let mut doc = vec!["Partial update for `update`: `None` leaves a column unchanged."];
    if serde {
        doc.extend([
            "",
            "From JSON, an absent field is `None` and `null` is `Some(None)`, which sets",
            "the column to `NULL`; serializing skips the `None` fields.",
        ]);
    }
    push_doc(output, 0, &doc);
    output.push_str("#[derive(Debug, Clone, Default, PartialEq)]\n");
    push_derives(output, &derives.input_derives());
    if serde {
        output.push_str("#[serde(default)]\n");
    }
    output.push_str(&format!("pub struct Update{} {{\n", table.struct_name));
    for column in columns {
        let doc = format!(
            "The new value of the {} column.",
            doc_name(&column.sql_name)
        );
        if column.nullable {
            push_doc(
                output,
                4,
                &[&doc, "`Some(None)` sets the column to `NULL`."],
            );
        } else {
            push_doc(output, 4, &[&doc]);
        }
        if serde {
            push_option_serde(output, column.nullable, false);
        }
        if validate {
            super::super::validate::push_field_rule(output, column);
        }
        output.push_str(&format!(
            "    pub {}: {},\n",
            column.rust_name,
            update_field_type(column)
        ));
    }
    output.push_str("}\n\n");

    output.push_str(&format!("impl Update{} {{\n", table.struct_name));
    push_doc(
        output,
        4,
        &["Returns `true` when the patch changes no column."],
    );
    output.push_str("    #[must_use]\n    pub fn is_empty(&self) -> bool {\n");
    let terms: Vec<String> = columns
        .iter()
        .map(|column| format!("self.{}.is_none()", column.rust_name))
        .collect();
    // rustfmt splits a call chain longer than its `chain_width` one call per
    // line: a field name near PostgreSQL's 63-byte limit is that long on its
    // own. The first term keeps `self.field` together; after `&&`, `self`
    // stands alone.
    if terms.iter().any(|term| term.len() > CHAIN_WIDTH) {
        let is_none = [".is_none()".to_owned()];
        for (index, column) in columns.iter().enumerate() {
            let name = &column.rust_name;
            if index == 0 {
                push_chain(output, 8, "", &format!("self.{name}"), &is_none);
            } else {
                let calls = [format!(".{name}"), is_none[0].clone()];
                push_chain(output, 12, "&& ", "self", &calls);
            }
        }
    } else {
        push_and_chain(output, 8, "", &terms, "");
    }
    output.push_str("    }\n}\n");
}
