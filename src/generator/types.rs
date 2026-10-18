//! `db/types.rs`: the Rust enums for the schema's enum types and value-list
//! columns, shared by every resource and query module.
//!
//! An enum type's enum derives `sqlx::Type` under the database type name,
//! keeping its schema when qualified, and each variant keeps its exact
//! database label through a per-variant rename, so labels of any case
//! round-trip. A value-list column's enum encodes and decodes as text,
//! keeping each exact value; text outside the list does not decode.

use crate::schema::EnumType;

use super::sql::{ident, str_literal};
use super::{DeriveOptions, HEADER, MAX_WIDTH, doc_name, push_derives, push_doc};

/// With `serde`, every enum also serializes each variant as its exact
/// database value.
pub fn render(enums: &[EnumType], derives: DeriveOptions) -> String {
    let mut output = format!(
        "{HEADER}\n//! Enum types and value-list columns from the schema; each variant keeps its\n//! exact database value.\n"
    );
    for enum_type in enums {
        output.push('\n');
        match &enum_type.column {
            None => render_enum_type(&mut output, enum_type, derives),
            Some(column) => render_value_list(&mut output, enum_type, column, derives),
        }
    }
    output
}

/// `#[serde(rename = "label")]` for a variant when `serde` is on.
fn push_serde_rename(output: &mut String, label: &str, serde: bool) {
    if serde {
        output.push_str(&format!("    #[serde(rename = {})]\n", str_literal(label)));
    }
}

fn render_enum_type(output: &mut String, enum_type: &EnumType, derives: DeriveOptions) {
    let serde = derives.serde;
    push_doc(
        output,
        0,
        &[&format!("The `{}` enum type.", enum_type.display_name())],
    );
    output.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, sqlx::Type)]\n");
    push_derives(output, &derives.value_derives());
    output.push_str(&format!(
        "#[sqlx(type_name = {})]\n",
        str_literal(&type_name(enum_type))
    ));
    output.push_str(&format!("pub enum {} {{\n", enum_type.rust_name));
    for variant in &enum_type.variants {
        push_doc(output, 4, &[&doc_name(&variant.label)]);
        push_serde_rename(output, &variant.label, serde);
        output.push_str(&format!(
            "    #[sqlx(rename = {})]\n    {},\n",
            str_literal(&variant.label),
            variant.rust_name
        ));
    }
    output.push_str("}\n");
}

/// A text enum: variants, `as_str`, `FromStr`, and SQLx impls that delegate
/// to `str`, so it binds and decodes wherever text does (also in arrays, as
/// set filters bind them).
fn render_value_list(
    output: &mut String,
    enum_type: &EnumType,
    column: &str,
    derives: DeriveOptions,
) {
    let serde = derives.serde;
    let name = &enum_type.rust_name;
    push_doc(
        output,
        0,
        &[&format!("The values of the `{column}` column.")],
    );
    output.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    push_derives(output, &derives.value_derives());
    output.push_str(&format!("pub enum {name} {{\n"));
    for variant in &enum_type.variants {
        push_doc(output, 4, &[&doc_name(&variant.label)]);
        push_serde_rename(output, &variant.label, serde);
        output.push_str(&format!("    {},\n", variant.rust_name));
    }
    output.push_str("}\n\n");

    output.push_str(&format!("impl {name} {{\n"));
    push_doc(output, 4, &["The column whose values this enum lists."]);
    output.push_str(&format!(
        "    pub const COLUMN: &str = {};\n\n",
        str_literal(column)
    ));
    push_doc(output, 4, &["The exact database value."]);
    output.push_str(
        "    #[must_use]\n    pub fn as_str(self) -> &'static str {\n        match self {\n",
    );
    for variant in &enum_type.variants {
        push_arm(
            output,
            &format!("Self::{}", variant.rust_name),
            &str_literal(&variant.label),
        );
    }
    output.push_str("        }\n    }\n}\n\n");

    output.push_str(&format!(
        "impl std::str::FromStr for {name} {{\n    type Err = String;\n\n    fn from_str(value: &str) -> Result<Self, Self::Err> {{\n        match value {{\n"
    ));
    for variant in &enum_type.variants {
        push_arm(
            output,
            &str_literal(&variant.label),
            &format!("Ok(Self::{})", variant.rust_name),
        );
    }
    // The message names the column through `COLUMN`, so the arm's width,
    // and with it rustfmt's layout, is the same for every enum.
    output.push_str(
        "            other => Err(format!(\"`{other}` is not a value of `{}`\", Self::COLUMN)),\n        }\n    }\n}\n\n",
    );

    output.push_str(&format!(
        "impl sqlx::Type<sqlx::Postgres> for {name} {{
    fn type_info() -> sqlx::postgres::PgTypeInfo {{
        <str as sqlx::Type<sqlx::Postgres>>::type_info()
    }}

    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {{
        <str as sqlx::Type<sqlx::Postgres>>::compatible(ty)
    }}
}}

impl sqlx::postgres::PgHasArrayType for {name} {{
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {{
        <&str as sqlx::postgres::PgHasArrayType>::array_type_info()
    }}

    fn array_compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {{
        <&str as sqlx::postgres::PgHasArrayType>::array_compatible(ty)
    }}
}}

impl sqlx::Encode<'_, sqlx::Postgres> for {name} {{
    fn encode_by_ref(
        &self,
        buf: &mut sqlx::postgres::PgArgumentBuffer,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {{
        <&str as sqlx::Encode<'_, sqlx::Postgres>>::encode(self.as_str(), buf)
    }}
}}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for {name} {{
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {{
        Ok(<&str as sqlx::Decode<'r, sqlx::Postgres>>::decode(value)?.parse()?)
    }}
}}
"
    ));
}

/// Appends a match arm at the depth of a method body's `match`, wrapping
/// the body in a block when the arm is too wide, as rustfmt does.
fn push_arm(output: &mut String, pattern: &str, body: &str) {
    let arm = format!("            {pattern} => {body},\n");
    if arm.len() - 1 <= MAX_WIDTH {
        output.push_str(&arm);
    } else {
        output.push_str(&format!(
            "            {pattern} => {{\n                {body}\n            }}\n"
        ));
    }
}

/// The type name as PostgreSQL resolves it (`app.ticket_state`,
/// `"Mixed Case"`), which SQLx uses to look the type up.
fn type_name(enum_type: &EnumType) -> String {
    match &enum_type.schema {
        Some(schema) => format!("{}.{}", ident(schema), ident(&enum_type.sql_name)),
        None => ident(&enum_type.sql_name),
    }
}
