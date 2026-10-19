// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `account` request and response bodies.

use crate::db::types::Role;
use crate::db::users::models::{User, UserId};

/// The longest display name, in characters.
pub const DISPLAY_NAME_MAX: u64 = 100;

/// `PATCH /users/me`'s body. A field left out keeps its value; the display
/// name is trimmed, and `null` or an empty one clears it.
#[derive(
    Debug, Clone, Default, PartialEq, serde::Deserialize, validator::Validate, utoipa::ToSchema,
)]
#[serde(default)]
pub struct UpdateProfileDto {
    /// The new display name, at most 100 characters.
    #[serde(deserialize_with = "trimmed")]
    #[validate(length(
        max = DISPLAY_NAME_MAX,
        message = "display_name must be at most 100 characters"
    ))]
    pub display_name: Option<Option<String>>,
}

/// A `users` row as the API answers it: the same shape as the `users`
/// feature's, which this feature may not import.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct UserResponse {
    /// The account's id.
    #[schema(value_type = uuid::Uuid)]
    pub id: UserId,
    /// The email address.
    pub email: String,
    /// The display name.
    pub display_name: Option<String>,
    /// Whether the account has a password (an invited one has none yet).
    pub has_password: bool,
    /// The role.
    pub role: Role,
    /// Whether the account may sign in.
    pub is_active: bool,
    /// Whether the email address is verified.
    pub email_verified: bool,
    /// Whether two-factor sign-in is on.
    pub totp_enabled: bool,
    /// When the account was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<User> for UserResponse {
    fn from(row: User) -> Self {
        UserResponse {
            id: row.id,
            email: row.email,
            display_name: row.display_name,
            has_password: row.has_password,
            role: row.role,
            is_active: row.is_active,
            email_verified: row.email_verified,
            totp_enabled: row.totp_enabled,
            created_at: row.created_at,
        }
    }
}

/// Deserializes a present display name: `null` and an empty (after
/// trimming) one are `Some(None)`, anything else `Some(Some(trimmed))`.
fn trimmed<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: Option<String> = serde::Deserialize::deserialize(deserializer)?;
    let value = value
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty());
    Ok(Some(value))
}
