use crate::schema::{Column, Table, mentions_generated_type};

use super::{
    HEADER, doc_name, insert_field_type, keys, push_and_chain, push_doc, push_serde_derive,
    update_field_type, use_paths,
};

/// The doc line of a row, input, or patch field: the column it holds.
fn column_doc(column: &Column) -> String {
    format!("The {} column.", doc_name(&column.sql_name))
}

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

/// The generated `db/nullable.rs` deserializer, from a resource's `models.rs`.
const NULLABLE_DESERIALIZE: &str = "super::super::nullable::deserialize";

/// Renders `db/nullable.rs`, generated with `derive = ["serde"]`: the
/// deserializer telling an absent `Option<Option<T>>` field from `null`.
pub fn render_nullable() -> String {
    format!("{HEADER}\n{NULLABLE}")
}

const NULLABLE: &str = r"//! Serde support for `Option<Option<T>>` fields, which tell an absent
//! field (`None`) from `null` (`Some(None)`).

use serde::{Deserialize, Deserializer};

/// Deserializes a field that is present, `null` included, as `Some`: `null`
/// becomes `Some(None)` and a value `Some(Some(value))`. An absent field never
/// reaches this function; `#[serde(default)]` makes it `None`.
///
/// # Errors
///
/// The deserializer's error when the field is neither `null` nor a `T`.
pub fn deserialize<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}
";

pub fn render(table: &Table, serde: bool) -> String {
    let mut output = format!("{HEADER}\n");
    let mut imports = keys::id_imports(table, &table.columns, true);
    if table
        .columns
        .iter()
        .any(|column| mentions_generated_type(&column.rust_type))
    {
        imports.push("super::super::types".to_owned());
    }
    if !imports.is_empty() {
        output.push_str(&use_paths(&imports));
        output.push('\n');
    }
    keys::render_key_type(&mut output, table, serde);
    render_row(&mut output, table, serde);
    keys::render_row_key(&mut output, table);

    let insert_columns = table.insert_columns();
    if !insert_columns.is_empty() {
        render_insert_input(&mut output, table, &insert_columns, serde);
    }

    let update_columns = table.update_columns();
    if !table.primary_key.is_empty() && !update_columns.is_empty() {
        render_update_patch(&mut output, table, &update_columns, serde);
    }

    output
}

fn render_row(output: &mut String, table: &Table, serde: bool) {
    let doc = format!("A row of the `{}` table.", table.display_name());
    push_doc(output, 0, &[&doc]);
    output.push_str("#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]\n");
    push_serde_derive(output, serde);
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

fn render_insert_input(output: &mut String, table: &Table, columns: &[&Column], serde: bool) {
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
        .all(|column| column.is_omittable_on_insert() || column.nullable);
    if all_optional {
        output.push_str("#[derive(Debug, Clone, Default, PartialEq)]\n");
    } else {
        output.push_str("#[derive(Debug, Clone, PartialEq)]\n");
    }
    push_serde_derive(output, serde);
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
        } else {
            push_doc(output, 4, &[&doc]);
        }
        // Plain serde reads both an absent field and `null` as `None`, which
        // would make `Some(None)` unreachable from JSON.
        if serde && column.is_omittable_on_insert() && column.nullable {
            push_option_serde(output, true, true);
        }
        output.push_str(&format!(
            "    pub {}: {},\n",
            column.rust_name,
            insert_field_type(column)
        ));
    }
    output.push_str("}\n");
}

fn render_update_patch(output: &mut String, table: &Table, columns: &[&Column], serde: bool) {
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
    push_serde_derive(output, serde);
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
    // rustfmt splits a call chain longer than its `chain_width` of 60 one
    // call per line: a field name near PostgreSQL's 63-byte limit is that
    // long on its own.
    if terms.iter().any(|term| term.len() > 60) {
        for (index, (term, column)) in terms.iter().zip(columns).enumerate() {
            let name = &column.rust_name;
            output.push_str(&match (index, term.len() > 60) {
                (0, false) => format!("        {term}\n"),
                (0, true) => format!("        self.{name}\n            .is_none()\n"),
                (_, false) => format!("            && {term}\n"),
                (_, true) => format!(
                    "            && self\n                .{name}\n                .is_none()\n"
                ),
            });
        }
    } else {
        push_and_chain(output, 8, "", &terms, "");
    }
    output.push_str("    }\n}\n");
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{column, defaulted, generated, nullable, table};
    use super::*;

    fn render(table: &Table) -> String {
        super::render(table, false)
    }

    #[test]
    fn serde_derives_rows_inputs_and_patches_on_request() {
        let users = table(
            vec![column("id", "i64"), column("email", "String")],
            &["id"],
        );
        let serde = "serde::Serialize, serde::Deserialize)]\n";
        assert!(!render(&users).contains("serde"));
        let rendered = super::render(&users, true);
        for item in ["", "pub struct User {", "pub struct NewUser {"] {
            assert!(
                rendered.contains(&format!("{serde}{item}")),
                "{item}:\n{rendered}"
            );
        }
        assert!(
            rendered.contains(&format!(
                "{serde}#[serde(default)]\npub struct UpdateUser {{"
            )),
            "{rendered}"
        );
    }

    /// Plain serde reads both an absent field and `null` as `None`; the
    /// generated `db::nullable` helper reads `null` as `Some(None)`.
    #[test]
    fn serde_patches_and_inputs_tell_absent_from_null() {
        let users = table(
            vec![
                column("id", "i64"),
                column("email", "String"),
                nullable("note", "String"),
                defaulted(nullable("tag", "String")),
            ],
            &["id"],
        );
        let rendered = super::render(&users, true);
        let patch = rendered.split("pub struct UpdateUser {").nth(1).unwrap();
        assert!(patch.contains(
            "    #[serde(skip_serializing_if = \"Option::is_none\")]\n    pub email: Option<String>,\n"
        ), "{patch}");
        assert!(patch.contains(
            "    #[serde(\n        deserialize_with = \"super::super::nullable::deserialize\",\n        skip_serializing_if = \"Option::is_none\"\n    )]\n    pub note: Option<Option<String>>,\n"
        ), "{patch}");

        let input = rendered.split("pub struct NewUser {").nth(1).unwrap();
        let input = input.split("\n}\n").next().unwrap();
        assert!(input.contains(
            "    #[serde(\n        default,\n        deserialize_with = \"super::super::nullable::deserialize\",\n        skip_serializing_if = \"Option::is_none\"\n    )]\n    pub tag: Option<Option<String>>,"
        ), "{input}");
        // Only the `Option<Option<T>>` input field needs the helper.
        assert_eq!(input.matches("#[serde(").count(), 1, "{input}");
    }

    #[test]
    fn nullable_helper_reads_null_as_some_none() {
        let rendered = render_nullable();
        assert!(rendered.starts_with(HEADER));
        assert!(rendered.contains(
            "pub fn deserialize<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>"
        ));
        assert!(rendered.contains("Option::<T>::deserialize(deserializer).map(Some)"));
    }

    #[test]
    fn renders_row_struct_with_from_row() {
        let rendered = render(&table(vec![column("email", "String")], &[]));
        assert!(rendered.starts_with("// @generated by rok-sqlc. Do not edit by hand.\n"));
        assert!(rendered.contains(
            "/// A row of the `users` table.\n#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]\npub struct User {\n    /// The `email` column.\n    pub email: String,\n}"
        ));
    }

    #[test]
    fn row_doc_names_schema_qualified_table() {
        let mut users = table(vec![column("email", "String")], &[]);
        users.schema = Some("core".to_owned());
        assert!(render(&users).contains("/// A row of the `core.users` table."));
    }

    #[test]
    fn wraps_nullable_columns_in_option() {
        let rendered = render(&table(vec![nullable("email", "String")], &[]));
        assert!(rendered.contains("pub email: Option<String>,"));
    }

    #[test]
    fn emits_rename_for_renamed_fields() {
        let mut renamed = column("type", "String");
        renamed.rust_name = "type_".to_owned();
        let mut quoted = column("Mixed \"Case\"", "String");
        quoted.rust_name = "mixed_case".to_owned();
        let rendered = render(&table(vec![renamed, quoted], &[]));
        assert!(rendered.contains("    #[sqlx(rename = \"type\")]\n    pub type_: String,"));
        assert!(
            rendered.contains(
                "    #[sqlx(rename = r#\"Mixed \"Case\"\"#)]\n    pub mixed_case: String,"
            )
        );
    }

    #[test]
    fn insert_input_omits_generated_and_wraps_defaulted_columns() {
        let rendered = render(&table(
            vec![
                generated(column("id", "i64")),
                column("email", "String"),
                nullable("nickname", "String"),
                defaulted(column("created_at", "chrono::DateTime<chrono::Utc>")),
                defaulted(nullable("tag", "String")),
            ],
            &["id"],
        ));
        let input = rendered.split("pub struct NewUser {").nth(1).unwrap();
        let input = input.split('}').next().unwrap();
        assert!(!input.contains("pub id:"));
        assert!(input.contains("    pub email: String,\n"));
        assert!(input.contains("    pub nickname: Option<String>,\n"));
        assert!(input.contains("    pub created_at: Option<chrono::DateTime<chrono::Utc>>,\n"));
        assert!(input.contains(
            "    /// `None` applies the column default; `Some(None)` inserts `NULL`.\n    pub tag: Option<Option<String>>,\n"
        ));
        assert!(rendered.contains("#[derive(Debug, Clone, PartialEq)]\npub struct NewUser {"));
    }

    #[test]
    fn insert_input_derives_default_when_every_field_is_optional() {
        let rendered = render(&table(
            vec![
                generated(column("id", "i64")),
                defaulted(column("hits", "i32")),
                nullable("note", "String"),
            ],
            &["id"],
        ));
        assert!(
            rendered.contains("#[derive(Debug, Clone, Default, PartialEq)]\npub struct NewUser {")
        );
    }

    #[test]
    fn update_patch_documents_null_and_offers_is_empty() {
        let rendered = render(&table(
            vec![
                column("id", "i64"),
                column("email", "String"),
                nullable("note", "String"),
            ],
            &["id"],
        ));
        assert!(
            rendered
                .contains("#[derive(Debug, Clone, Default, PartialEq)]\npub struct UpdateUser {")
        );
        assert!(rendered.contains("    pub email: Option<String>,\n"));
        assert!(rendered.contains(
            "    /// `Some(None)` sets the column to `NULL`.\n    pub note: Option<Option<String>>,\n"
        ));
        assert!(!rendered.contains("    pub id: Option"));
        assert!(rendered.contains(
            "    pub fn is_empty(&self) -> bool {\n        self.email.is_none() && self.note.is_none()\n    }"
        ));
    }

    #[test]
    fn omits_update_patch_without_primary_key() {
        let rendered = render(&table(vec![column("email", "String")], &[]));
        assert!(!rendered.contains("pub struct UpdateUser"));
    }

    #[test]
    fn output_is_deterministic() {
        let build = || {
            render(&table(
                vec![column("id", "i64"), column("email", "String")],
                &["id"],
            ))
        };
        assert_eq!(build(), build());
    }
}
