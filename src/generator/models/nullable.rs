//! `db/nullable.rs`, generated with `derive = ["serde"]`: the deserializer
//! telling an absent `Option<Option<T>>` field from `null`.

use super::super::HEADER;

/// The generated `db/nullable.rs` deserializer, from a resource's `models.rs`.
pub(super) const NULLABLE_DESERIALIZE: &str = "super::super::nullable::deserialize";

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
