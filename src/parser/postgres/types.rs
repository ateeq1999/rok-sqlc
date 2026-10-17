//! SQL types to Rust types.

use sqlparser::ast::{ArrayElemTypeDef, DataType, ExactNumberInfo, ObjectName, TimezoneInfo};

use super::enums::{EnumEntry, EnumRegistry};
use super::names::object_leaf_name;
use crate::schema::EnumType;
use crate::schema::rust_type;

/// Maps a query-file cast's SQL type to its Rust type, resolving the
/// database enum types among the generated `enums` (a value-list enum names
/// no database type).
pub(crate) fn map_type(
    data_type: &DataType,
    enums: &[EnumType],
) -> std::result::Result<(String, String), String> {
    let registry = EnumRegistry {
        entries: enums
            .iter()
            .filter(|enum_type| enum_type.column.is_none())
            .map(|enum_type| EnumEntry {
                enum_type: enum_type.clone(),
                broken: None,
            })
            .collect(),
    };
    map_type_in(data_type, &registry)
}

/// Maps a SQL type to `(Rust type, base type)`, resolving enum types defined
/// so far.
pub(super) fn map_type_in(
    data_type: &DataType,
    enums: &EnumRegistry,
) -> std::result::Result<(String, String), String> {
    let base = match data_type {
        DataType::SmallInt(_) | DataType::Int2(_) => "i16",
        DataType::Int(_) | DataType::Int4(_) | DataType::Integer(_) => "i32",
        DataType::BigInt(_) | DataType::Int8(_) => "i64",
        DataType::Bool | DataType::Boolean => "bool",
        DataType::Real => "f32",
        DataType::Float8 | DataType::DoublePrecision | DataType::Double(_) | DataType::Float(_) => {
            "f64"
        }
        DataType::Character(_)
        | DataType::Char(_)
        | DataType::CharacterVarying(_)
        | DataType::Varchar(_)
        | DataType::Text
        | DataType::String(_) => "String",
        DataType::Uuid => "uuid::Uuid",
        DataType::Date => "chrono::NaiveDate",
        DataType::Time(_, TimezoneInfo::None | TimezoneInfo::WithoutTimeZone) => {
            "chrono::NaiveTime"
        }
        DataType::Timestamp(_, TimezoneInfo::None | TimezoneInfo::WithoutTimeZone) => {
            "chrono::NaiveDateTime"
        }
        DataType::Timestamp(_, TimezoneInfo::Tz | TimezoneInfo::WithTimeZone) => {
            "chrono::DateTime<chrono::Utc>"
        }
        DataType::Time(_, TimezoneInfo::Tz | TimezoneInfo::WithTimeZone) => rust_type::TIME_TZ,
        DataType::Interval { .. } => rust_type::INTERVAL,
        DataType::Bytea => rust_type::BYTES,
        DataType::JSON | DataType::JSONB => "serde_json::Value",
        DataType::Numeric(info) | DataType::Decimal(info) => {
            check_decimal_precision(info)?;
            "rust_decimal::Decimal"
        }
        DataType::Array(element) => {
            let inner = array_element_type(element)?;
            let (base, _) = map_type_in(inner, enums)?;
            return Ok((format!("Vec<{base}>"), format!("Vec<{base}>")));
        }
        DataType::Custom(name, _) => match enums.lookup(name) {
            Some(enum_type) => {
                let path = enum_type.rust_path()?;
                return Ok((path.clone(), path));
            }
            None => custom_type_leaf(name)?,
        },
        other => return Err(format!("type `{other}` is not supported yet")),
    };

    Ok((base.to_owned(), base.to_owned()))
}

fn custom_type_leaf(name: &ObjectName) -> std::result::Result<&'static str, String> {
    match object_leaf_name(name).to_ascii_uppercase().as_str() {
        "SERIAL" => Ok("i32"),
        "BIGSERIAL" => Ok("i64"),
        "SMALLSERIAL" => Ok("i16"),
        "FLOAT4" => Ok("f32"),
        other => Err(format!("type `{other}` is not supported yet")),
    }
}

pub(super) fn is_serial_type(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::Custom(name, _)
            if matches!(
                object_leaf_name(name).to_ascii_uppercase().as_str(),
                "SERIAL" | "BIGSERIAL" | "SMALLSERIAL"
            )
    )
}

fn array_element_type(element: &ArrayElemTypeDef) -> std::result::Result<&DataType, String> {
    match element {
        ArrayElemTypeDef::SquareBracket(inner, _)
        | ArrayElemTypeDef::AngleBracket(inner)
        | ArrayElemTypeDef::Parenthesis(inner) => Ok(inner),
        ArrayElemTypeDef::None => Err("array type has no element type".to_owned()),
    }
}

const DECIMAL_MAX_PRECISION: u64 = 28;

fn check_decimal_precision(info: &ExactNumberInfo) -> std::result::Result<(), String> {
    let precision = match info {
        ExactNumberInfo::None => return Ok(()),
        ExactNumberInfo::Precision(precision)
        | ExactNumberInfo::PrecisionAndScale(precision, _) => *precision,
    };
    if precision > DECIMAL_MAX_PRECISION {
        return Err(format!(
            "NUMERIC precision {precision} exceeds rust_decimal's {DECIMAL_MAX_PRECISION} significant digits"
        ));
    }
    Ok(())
}
