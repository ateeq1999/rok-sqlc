//! `db/types.rs`: the Rust enums for the schema's enum types, shared by every
//! resource and query module.
//!
//! Each enum derives `sqlx::Type` under the database type name, keeping its
//! schema when qualified, and each variant keeps its exact database label
//! through a per-variant rename, so labels of any case round-trip.

use crate::schema::EnumType;

use super::sql::{ident, str_literal};
use super::{HEADER, push_doc};

pub fn render(enums: &[EnumType]) -> String {
    let mut output = format!(
        "{HEADER}\n// Enum types from the schema; each variant keeps its exact database label.\n"
    );
    for enum_type in enums {
        output.push('\n');
        push_doc(
            &mut output,
            0,
            &[&format!("The `{}` enum type.", enum_type.display_name())],
        );
        output.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, sqlx::Type)]\n");
        output.push_str(&format!(
            "#[sqlx(type_name = {})]\n",
            str_literal(&type_name(enum_type))
        ));
        output.push_str(&format!("pub enum {} {{\n", enum_type.rust_name));
        for variant in &enum_type.variants {
            output.push_str(&format!(
                "    #[sqlx(rename = {})]\n    {},\n",
                str_literal(&variant.label),
                variant.rust_name
            ));
        }
        output.push_str("}\n");
    }
    output
}

/// The type name as PostgreSQL resolves it (`app.ticket_state`,
/// `"Mixed Case"`), which SQLx uses to look the type up.
fn type_name(enum_type: &EnumType) -> String {
    match &enum_type.schema {
        Some(schema) => format!("{}.{}", ident(schema), ident(&enum_type.sql_name)),
        None => ident(&enum_type.sql_name),
    }
}
