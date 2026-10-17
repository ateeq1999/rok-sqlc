//! What a Rust type string denotes.
//!
//! Types travel through the model as the text they render as (`i64`,
//! `Option<String>`, `types::Status`, `CategoryId`): every renderer writes
//! them out verbatim, and query overrides may name any Rust type. Questions
//! about a type (is it optional, a list, text, a generated enum, a scalar a
//! typed id can wrap?) are answered here, so the string shapes are parsed in
//! one place (ADR 0013).

/// The shared generated module holding enum types. A column of an enum type
/// has the Rust type `types::{Name}`, and every generated file mentioning one
/// imports `types` along its path to the `db` module.
pub const TYPES_MODULE: &str = "types";

/// `INTERVAL`: SQLx's own type, which implements neither `PartialOrd` nor
/// serde's traits.
pub const INTERVAL: &str = "sqlx::postgres::types::PgInterval";

/// `BYTEA`.
pub const BYTES: &str = "Vec<u8>";

/// `TIME WITH TIME ZONE`: SQLx's own type, which implements neither
/// `PartialOrd` nor serde's traits. Its default parameters are `chrono`'s
/// `NaiveTime` and `FixedOffset`, or the `time` crate's types when SQLx's
/// `time` feature is on; the generated code compiles with either.
pub const TIME_TZ: &str = "sqlx::postgres::types::PgTimeTz";

/// `MONEY`: SQLx's own type, which implements neither `PartialOrd` nor
/// serde's traits.
pub const MONEY: &str = "sqlx::postgres::types::PgMoney";

/// A range type is `PgRange<T>`: SQLx's own type, which implements neither
/// `PartialOrd` nor serde's traits.
pub const RANGE: &str = "sqlx::postgres::types::PgRange";

/// `INET` and `CIDR`, through SQLx's `ipnetwork` feature.
pub const IP_NETWORK: &str = "ipnetwork::IpNetwork";

/// `MACADDR`, through SQLx's `mac_address` feature.
pub const MAC_ADDRESS: &str = "mac_address::MacAddress";

/// The outer shape of a Rust type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape<'a> {
    /// `Option<inner>`.
    Optional(&'a str),
    /// `Vec<item>`.
    List(&'a str),
    /// Any other type: one value.
    Value(&'a str),
}

impl<'a> Shape<'a> {
    pub fn of(rust_type: &'a str) -> Self {
        if let Some(inner) = generic_argument(rust_type, "Option") {
            Shape::Optional(inner)
        } else if let Some(item) = generic_argument(rust_type, "Vec") {
            Shape::List(item)
        } else {
            Shape::Value(rust_type)
        }
    }
}

/// `T` in `{wrapper}<T>`.
fn generic_argument<'a>(rust_type: &'a str, wrapper: &str) -> Option<&'a str> {
    rust_type
        .strip_prefix(wrapper)?
        .strip_prefix('<')?
        .strip_suffix('>')
}

/// What a column's base type (`Column::sqlx_type`, as the type mapping
/// produces it) is, for the decisions that depend on it: filtering, typed
/// ids, borrowing, conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseType {
    /// `i16`, `i32`, `i64`.
    Integer,
    /// `f32`, `f64`.
    Float,
    /// `rust_decimal::Decimal`.
    Decimal,
    Bool,
    /// `String`.
    Text,
    /// `uuid::Uuid`.
    Uuid,
    /// `chrono::NaiveDate`.
    Date,
    /// `chrono::NaiveTime`.
    Time,
    /// `chrono::NaiveDateTime`.
    Timestamp,
    /// `chrono::DateTime<chrono::Utc>`.
    TimestampTz,
    /// [`INTERVAL`].
    Interval,
    /// [`BYTES`], which is a list of bytes in Rust but one value in SQL.
    Bytes,
    /// [`TIME_TZ`].
    TimeTz,
    /// [`MONEY`].
    Money,
    /// [`RANGE`]`<T>`.
    Range,
    /// [`IP_NETWORK`].
    IpNetwork,
    /// [`MAC_ADDRESS`].
    MacAddress,
    /// A generated enum, `types::X`.
    Enum,
    /// An array, `Vec<T>`.
    Array,
    /// JSON (`serde_json::Value`) or any other type.
    Other,
}

impl BaseType {
    pub fn of(sqlx_type: &str) -> Self {
        match sqlx_type {
            "i16" | "i32" | "i64" => BaseType::Integer,
            "f32" | "f64" => BaseType::Float,
            "rust_decimal::Decimal" => BaseType::Decimal,
            "bool" => BaseType::Bool,
            "String" => BaseType::Text,
            "uuid::Uuid" => BaseType::Uuid,
            "chrono::NaiveDate" => BaseType::Date,
            "chrono::NaiveTime" => BaseType::Time,
            "chrono::NaiveDateTime" => BaseType::Timestamp,
            "chrono::DateTime<chrono::Utc>" => BaseType::TimestampTz,
            INTERVAL => BaseType::Interval,
            BYTES => BaseType::Bytes,
            TIME_TZ => BaseType::TimeTz,
            MONEY => BaseType::Money,
            IP_NETWORK => BaseType::IpNetwork,
            MAC_ADDRESS => BaseType::MacAddress,
            other if generic_argument(other, RANGE).is_some() => BaseType::Range,
            other if is_enum_path(other) => BaseType::Enum,
            other => match Shape::of(other) {
                Shape::List(_) => BaseType::Array,
                Shape::Optional(_) | Shape::Value(_) => BaseType::Other,
            },
        }
    }

    /// Timestamps, with or without time zone.
    pub fn is_timestamp(self) -> bool {
        matches!(self, BaseType::Timestamp | BaseType::TimestampTz)
    }

    /// Whether a typed id can wrap a value of this type: it must be `Eq`,
    /// `Ord`, `Hash`, and `Display`.
    pub fn supports_typed_id(self) -> bool {
        matches!(
            self,
            BaseType::Integer
                | BaseType::Bool
                | BaseType::Text
                | BaseType::Uuid
                | BaseType::Decimal
                | BaseType::Date
                | BaseType::Time
                | BaseType::Timestamp
                | BaseType::TimestampTz
        )
    }

    /// The inclusive bounds of the values an [`BaseType::Integer`] base type
    /// (`sqlx_type`) holds; `None` for any other type.
    pub fn integer_bounds(sqlx_type: &str) -> Option<(i128, i128)> {
        match sqlx_type {
            "i16" => Some((i128::from(i16::MIN), i128::from(i16::MAX))),
            "i32" => Some((i128::from(i32::MIN), i128::from(i32::MAX))),
            "i64" => Some((i128::from(i64::MIN), i128::from(i64::MAX))),
            _ => None,
        }
    }

    /// The binary mantissa digits of a [`BaseType::Float`] base type
    /// (`sqlx_type`); `None` for any other type.
    pub fn float_mantissa_digits(sqlx_type: &str) -> Option<u32> {
        match sqlx_type {
            "f32" => Some(f32::MANTISSA_DIGITS),
            "f64" => Some(f64::MANTISSA_DIGITS),
            _ => None,
        }
    }
}

/// Whether `rust_type` is a generated enum's path, `types::X`.
fn is_enum_path(rust_type: &str) -> bool {
    rust_type
        .strip_prefix(TYPES_MODULE)
        .and_then(|rest| rest.strip_prefix("::"))
        .is_some_and(|name| !name.is_empty() && !name.contains([':', '<', '>']))
}

/// Whether a Rust type holds an SQLx type that implements neither
/// `Serialize` nor `Deserialize` ([`INTERVAL`], [`TIME_TZ`], [`MONEY`],
/// [`RANGE`]), possibly inside `Option<…>` or `Vec<…>`.
pub fn lacks_serde(rust_type: &str) -> bool {
    [INTERVAL, TIME_TZ, MONEY, RANGE]
        .iter()
        .any(|sqlx_type| rust_type.contains(sqlx_type))
}

/// Whether a Rust type mentions a generated enum (`types::X`, possibly inside
/// `Option<…>` or `Vec<…>`), so its file must import the `types` module.
pub fn mentions_generated_type(rust_type: &str) -> bool {
    let needle = format!("{TYPES_MODULE}::");
    rust_type.match_indices(&needle).any(|(index, _)| {
        !rust_type[..index]
            .ends_with(|character: char| character.is_alphanumeric() || "_:".contains(character))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_and_base_types() {
        assert_eq!(Shape::of("Option<Vec<i32>>"), Shape::Optional("Vec<i32>"));
        assert_eq!(Shape::of("Vec<types::Mood>"), Shape::List("types::Mood"));
        assert_eq!(Shape::of("Optional<i32>"), Shape::Value("Optional<i32>"));
        assert_eq!(BaseType::of("types::Mood"), BaseType::Enum);
        assert_eq!(BaseType::of("Vec<types::Mood>"), BaseType::Array);
        assert_eq!(BaseType::of("serde_json::Value"), BaseType::Other);
        assert_eq!(
            BaseType::of("chrono::DateTime<chrono::Utc>"),
            BaseType::TimestampTz
        );
        assert_eq!(
            BaseType::of("sqlx::postgres::types::PgRange<chrono::DateTime<chrono::Utc>>"),
            BaseType::Range
        );
        assert_eq!(BaseType::of("ipnetwork::IpNetwork"), BaseType::IpNetwork);
        assert!(lacks_serde("Option<sqlx::postgres::types::PgRange<i32>>"));
        assert!(!lacks_serde("ipnetwork::IpNetwork"));
    }
}
