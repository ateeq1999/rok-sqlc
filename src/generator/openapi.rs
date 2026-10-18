//! What the OpenAPI document needs of the generated code (ADR 0016): the
//! utoipa 5 derives on the types an API feature exposes, the
//! `#[schema(value_type = …)]` of a field whose Rust type utoipa can't
//! describe, and the `utoipa` dependency hint.
//!
//! utoipa's `uuid`, `chrono` and `decimal` features describe those crates'
//! types, and it describes `serde_json::Value` as any value. A typed id is
//! described as its inner type, the network types as text. Inside a search
//! condition (`Ordered<T>`, `Equality<T>`), `T` must itself implement
//! `ToSchema`, which no foreign type does, so there it is text too.

use super::push_derives;
use crate::schema::rust_type::{IP_NETWORK, MAC_ADDRESS};
use crate::schema::{BaseType, Column, Shape, Table};

/// The derive of a type the document describes.
pub const TO_SCHEMA: &str = "utoipa::ToSchema";

/// The derive of a query string the document describes.
pub const INTO_PARAMS: &str = "utoipa::IntoParams";

/// The attribute placing an `IntoParams` type's fields in the query.
pub const IN_QUERY: &str = "#[into_params(parameter_in = Query)]\n";

/// [`TO_SCHEMA`] when `openapi` is on, otherwise nothing.
pub fn to_schema(openapi: bool) -> &'static [&'static str] {
    if openapi { &[TO_SCHEMA] } else { &[] }
}

/// With `openapi`, adds [`TO_SCHEMA`] to the `#[derive(...)]` line `output`
/// ends with.
pub fn push_to_schema(output: &mut String, openapi: bool) {
    push_derives(output, to_schema(openapi));
}

/// With `openapi`, adds [`INTO_PARAMS`] to the `#[derive(...)]` line
/// `output` ends with, and places the fields in the query.
pub fn push_into_params(output: &mut String, openapi: bool) {
    if openapi {
        push_derives(output, &[INTO_PARAMS]);
        output.push_str(IN_QUERY);
    }
}

/// The `value_type` of a field holding `column`'s value as `field_type` (its
/// value type, possibly in `Option` or `Option<Option<…>>`), or `None` when
/// utoipa describes `field_type` itself.
pub fn value_type(column: &Column, field_type: &str) -> Option<String> {
    let value = column.value_type();
    let described = if column.typed_id.is_some() {
        column.sqlx_type.clone()
    } else {
        as_text(&column.sqlx_type)
    };
    (described != value).then(|| field_type.replacen(value, &described, 1))
}

/// The `value_type` of a search condition field over `column`, such as
/// `Vec<Ordered<chrono::NaiveDate>>`, or `None` when the condition's value
/// implements `ToSchema`: a typed id, an enum, or a primitive.
pub fn condition_value_type(column: &Column, field_type: &str) -> Option<String> {
    if column.typed_id.is_some() {
        return None;
    }
    match column.base_type() {
        BaseType::Integer
        | BaseType::Float
        | BaseType::Bool
        | BaseType::Text
        | BaseType::Bytes
        | BaseType::Enum => None,
        _ => Some(field_type.replacen(&column.sqlx_type, "String", 1)),
    }
}

/// `    #[schema(value_type = …)]` for a field, with `openapi` and when
/// `value_type` is one.
pub fn push_value_type(output: &mut String, openapi: bool, value_type: Option<String>) {
    if let Some(value_type) = value_type.filter(|_| openapi) {
        output.push_str(&format!("    #[schema(value_type = {value_type})]\n"));
    }
}

/// `rust_type` with the types utoipa can't describe, the network types,
/// written as `String`: they serialize as text.
fn as_text(rust_type: &str) -> String {
    [IP_NETWORK, MAC_ADDRESS]
        .iter()
        .fold(rust_type.to_owned(), |text, foreign| {
            text.replace(foreign, "String")
        })
}

/// The data layer's `mod.rs` hint for Swagger UI, which `app.rs` serves the
/// document with. `vendored` bundles Swagger UI, so its build script doesn't
/// download it.
pub const SWAGGER_UI_HINT: &str =
    "//   utoipa-swagger-ui = { version = \"9\", features = [\"actix-web\", \"vendored\"] }\n";

/// The data layer's `mod.rs` hint for utoipa, with the features describing
/// the UUID, date and time, and decimal columns of `tables`.
pub fn dependency_hint(tables: &[Table]) -> String {
    let mut features = Vec::new();
    let mut uses = |feature: &'static str| {
        if !features.contains(&feature) {
            features.push(feature);
        }
    };
    for column in tables.iter().flat_map(|table| &table.columns) {
        let element = match Shape::of(&column.sqlx_type) {
            Shape::List(item) => BaseType::of(item),
            _ => column.base_type(),
        };
        match element {
            BaseType::Uuid => uses("uuid"),
            BaseType::Date | BaseType::Time | BaseType::Timestamp | BaseType::TimestampTz => {
                uses("chrono");
            }
            BaseType::Decimal => uses("decimal"),
            _ => {}
        }
    }
    features.sort_unstable();
    if features.is_empty() {
        "utoipa = \"5\"".to_owned()
    } else {
        let quoted: Vec<String> = features.iter().map(|name| format!("\"{name}\"")).collect();
        format!(
            "utoipa = {{ version = \"5\", features = [{}] }}",
            quoted.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::test_support::{column, nullable};
    use crate::schema::TypedId;

    #[test]
    fn typed_ids_are_described_as_their_inner_type() {
        let mut id = nullable("note_id", "uuid::Uuid");
        id.typed_id = Some(TypedId {
            module: "notes".into(),
            name: "NoteId".into(),
        });
        assert_eq!(
            value_type(&id, "Option<Option<NoteId>>").as_deref(),
            Some("Option<Option<uuid::Uuid>>")
        );
        assert_eq!(condition_value_type(&id, "Vec<Equality<NoteId>>"), None);
    }

    #[test]
    fn network_types_are_text_and_known_types_need_nothing() {
        let host = nullable("hosts", "Vec<ipnetwork::IpNetwork>");
        assert_eq!(
            value_type(&host, "Option<Vec<ipnetwork::IpNetwork>>").as_deref(),
            Some("Option<Vec<String>>")
        );
        for rust_type in [
            "chrono::NaiveDate",
            "rust_decimal::Decimal",
            "serde_json::Value",
        ] {
            assert_eq!(value_type(&column("c", rust_type), rust_type), None);
        }
    }

    #[test]
    fn conditions_over_foreign_values_are_text() {
        let at = column("at", "chrono::DateTime<chrono::Utc>");
        assert_eq!(
            condition_value_type(&at, "Vec<Ordered<chrono::DateTime<chrono::Utc>>>").as_deref(),
            Some("Vec<Ordered<String>>")
        );
        for rust_type in ["i32", "f64", "bool", "Vec<u8>", "types::Mood"] {
            let field = format!("Vec<Equality<{rust_type}>>");
            assert_eq!(condition_value_type(&column("c", rust_type), &field), None);
        }
    }
}
