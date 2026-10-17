use crate::schema::{Column, Table, mentions_generated_type};

use super::{
    CHAIN_WIDTH, HEADER, SERDE_DERIVES, doc_name, insert_field_type, keys, push_and_chain,
    push_chain, push_derives, push_doc, push_serde_derive, update_field_type, use_paths,
};

/// The derives a create input or update patch adds: serde's, and with
/// `validate` (ADR 0015) `validator::Validate`.
fn input_derives(serde: bool, validate: bool) -> Vec<&'static str> {
    let mut derives = Vec::new();
    if serde {
        derives.extend(SERDE_DERIVES);
    }
    if validate {
        derives.push("validator::Validate");
    }
    derives
}

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

pub fn render(table: &Table, serde: bool, validate: bool) -> String {
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
    keys::render_key_type(&mut output, table, serde, validate);
    render_row(&mut output, table, serde);
    keys::render_row_key(&mut output, table);

    let insert_columns = table.insert_columns();
    if !insert_columns.is_empty() {
        render_insert_input(&mut output, table, &insert_columns, serde, validate);
    }

    let mut update_columns = table.update_columns();
    if !table.primary_key.is_empty() && !update_columns.is_empty() {
        render_update_patch(&mut output, table, &update_columns, serde, validate);
    } else {
        update_columns.clear();
    }

    if validate {
        let fields = insert_columns.into_iter().chain(update_columns);
        super::validate::push_decimal_checks(&mut output, fields);
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

fn render_insert_input(
    output: &mut String,
    table: &Table,
    columns: &[&Column],
    serde: bool,
    validate: bool,
) {
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
    push_derives(output, &input_derives(serde, validate));
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
            super::validate::push_field_rule(output, column);
        }
        output.push_str(&format!(
            "    pub {}: {},\n",
            column.rust_name,
            insert_field_type(column)
        ));
    }
    output.push_str("}\n");
}

fn render_update_patch(
    output: &mut String,
    table: &Table,
    columns: &[&Column],
    serde: bool,
    validate: bool,
) {
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
    push_derives(output, &input_derives(serde, validate));
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
            super::validate::push_field_rule(output, column);
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

#[cfg(test)]
mod tests {
    use super::super::test_support::{column, defaulted, generated, nullable, table};
    use super::*;
    use crate::schema::{LengthRange, NumberBound, TypedId, ValueRange};

    fn render(table: &Table) -> String {
        super::render(table, false, false)
    }

    #[test]
    fn serde_derives_rows_inputs_and_patches_on_request() {
        let users = table(
            vec![column("id", "i64"), column("email", "String")],
            &["id"],
        );
        let serde = "serde::Serialize, serde::Deserialize)]\n";
        assert!(!render(&users).contains("serde"));
        let rendered = super::render(&users, true, false);
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

    /// `validate` derives `validator::Validate` for the create input and the
    /// update patch, and for nothing else (ADR 0015).
    #[test]
    fn validate_derives_only_inputs_and_patches() {
        let users = table(
            vec![column("id", "i64"), column("email", "String")],
            &["id"],
        );
        let rendered = super::render(&users, true, true);
        let derive = "serde::Serialize, serde::Deserialize, validator::Validate)]\n";
        assert!(
            rendered.contains(&format!("{derive}pub struct NewUser {{")),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "serde::Deserialize, validator::Validate,\n)]\n#[serde(default)]\npub struct UpdateUser {"
            ),
            "{rendered}"
        );
        assert_eq!(rendered.matches("validator::Validate").count(), 2);
        assert!(!super::render(&users, true, false).contains("validat"));
    }

    /// A text column's declared length and length checks become a `length`
    /// rule, counted in characters, on the input and the patch alike.
    #[test]
    fn validate_puts_length_rules_on_text_fields() {
        let lengths = |declared: Option<u32>, min: Option<u32>, max: Option<u32>| {
            move |mut column: Column| {
                column.declared_length = declared;
                if min.is_some() || max.is_some() {
                    column.length_range = Some(LengthRange { min, max });
                }
                column
            }
        };
        let users = table(
            vec![
                column("id", "i64"),
                lengths(Some(80), None, None)(column("name", "String")),
                lengths(Some(80), Some(1), Some(100))(column("title", "String")),
                lengths(None, Some(3), None)(nullable("code", "String")),
                lengths(Some(1), None, None)(column("grade", "String")),
                lengths(Some(3), Some(3), None)(column("iso", "String")),
                column("body", "String"),
            ],
            &["id"],
        );
        let rendered = super::render(&users, true, true);
        let (input, patch) = rendered
            .split_once("pub struct UpdateUser {")
            .expect("a patch");
        let rules = [
            (
                "name",
                "#[validate(length(max = 80, message = \"name must be at most 80 characters\"))]",
            ),
            (
                "title",
                "#[validate(length(\n        min = 1,\n        max = 80,\n        message = \"title must be between 1 and 80 characters\"\n    ))]",
            ),
            (
                "code",
                "#[validate(length(min = 3, message = \"code must be at least 3 characters\"))]",
            ),
            (
                "grade",
                "#[validate(length(max = 1, message = \"grade must be at most 1 character\"))]",
            ),
            (
                "iso",
                "#[validate(length(equal = 3, message = \"iso must be exactly 3 characters\"))]",
            ),
        ];
        for (field, rule) in rules {
            for part in [input, patch] {
                let expected = format!("    {rule}\n    pub {field}: ");
                assert!(part.contains(&expected), "{expected}\n{part}");
            }
        }
        assert_eq!(rendered.matches("#[validate(").count(), 10, "{rendered}");
        assert!(!super::render(&users, true, false).contains("validat"));
    }

    /// A numeric column's range checks become a `range` rule; a decimal one,
    /// which `validator` cannot compare, a `custom` rule with the stock
    /// `range` code, checked by a function in `models.rs`.
    #[test]
    fn validate_puts_range_rules_on_numeric_fields() {
        let bound = |value: &str, exclusive: bool| NumberBound {
            value: value.to_owned(),
            exclusive,
        };
        let ranged = |min: Option<NumberBound>, max: Option<NumberBound>| {
            move |mut column: Column| {
                column.value_range = Some(ValueRange { min, max });
                column
            }
        };
        let mut id = column("id", "i64");
        id.typed_id = Some(TypedId {
            module: "users".to_owned(),
            name: "UserId".to_owned(),
        });
        let users = table(
            vec![
                ranged(Some(bound("1", false)), None)(id),
                ranged(Some(bound("0", false)), None)(column("qty", "i32")),
                ranged(Some(bound("1", false)), Some(bound("5", false)))(nullable("rating", "i16")),
                ranged(Some(bound("0", true)), Some(bound("1", false)))(column("score", "f64")),
                ranged(None, Some(bound("-0.5", true)))(column("ratio", "f32")),
                ranged(Some(bound("0", false)), Some(bound("99.95", true)))(column(
                    "price",
                    "rust_decimal::Decimal",
                )),
            ],
            &["id"],
        );
        let rendered = super::render(&users, true, true);
        let (input, patch) = rendered
            .split_once("pub struct UpdateUser {")
            .expect("a patch");
        let rules = [
            (
                "qty",
                "#[validate(range(min = 0, message = \"qty must be at least 0\"))]",
            ),
            (
                "rating",
                "#[validate(range(min = 1, max = 5, message = \"rating must be between 1 and 5\"))]",
            ),
            (
                "score",
                "#[validate(range(\n        exclusive_min = 0.0,\n        max = 1.0,\n        message = \"score must be greater than 0 and at most 1\"\n    ))]",
            ),
            (
                "ratio",
                "#[validate(range(exclusive_max = -0.5, message = \"ratio must be less than -0.5\"))]",
            ),
            (
                "price",
                "#[validate(custom(\n        function = \"price_in_range\",\n        code = \"range\",\n        message = \"price must be at least 0 and less than 99.95\"\n    ))]",
            ),
        ];
        for (field, rule) in rules {
            for part in [input, patch] {
                let expected = format!("    {rule}\n    pub {field}: ");
                assert!(part.contains(&expected), "{expected}\n{part}");
            }
        }
        // The typed id field keeps its range to the database.
        assert_eq!(rendered.matches("#[validate(").count(), 10, "{rendered}");
        assert!(
            rendered.ends_with(
                "/// The range rule of the `price` column.
fn price_in_range(value: &rust_decimal::Decimal) -> Result<(), validator::ValidationError> {
    if *value < rust_decimal::Decimal::from_i128_with_scale(0, 0)
        || *value >= rust_decimal::Decimal::from_i128_with_scale(9995, 2)
    {
        return Err(validator::ValidationError::new(\"range\"));
    }
    Ok(())
}
"
            ),
            "{rendered}"
        );
        assert!(!super::render(&users, true, false).contains("validat"));
    }

    /// A typed id over text measures the text inside, so a key column, or a
    /// foreign key using the id, keeps its length rule.
    #[test]
    fn validate_makes_text_typed_ids_measurable() {
        let typed = |mut column: Column, name: &str, module: &str| {
            column.typed_id = Some(TypedId {
                module: module.to_owned(),
                name: name.to_owned(),
            });
            column.declared_length = Some(12);
            column
        };
        let users = table(
            vec![
                typed(column("code", "String"), "UserId", "users"),
                typed(nullable("team_code", "String"), "TeamId", "teams"),
            ],
            &["code"],
        );
        let rendered = super::render(&users, true, true);
        assert!(
            rendered.contains(
                "        std::fmt::Display::fmt(&self.0, formatter)
    }
}

impl validator::ValidateLength<u64> for UserId {
    fn length(&self) -> Option<u64> {
        validator::ValidateLength::length(&self.0)
    }
}
"
            ),
            "{rendered}"
        );
        assert_eq!(
            rendered.matches("impl validator::").count(),
            1,
            "{rendered}"
        );
        for field in ["code: UserId", "team_code: Option<TeamId>"] {
            assert!(
                rendered.contains(&format!(
                    "message = \"{} must be at most 12 characters\"))]\n    pub {field}",
                    field.split(':').next().unwrap()
                )),
                "{field}\n{rendered}"
            );
        }
        assert!(!super::render(&users, true, false).contains("validat"));

        let numbered = table(vec![typed(column("id", "i64"), "UserId", "users")], &["id"]);
        assert!(!super::render(&numbered, true, true).contains("ValidateLength"));
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
        let rendered = super::render(&users, true, false);
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
