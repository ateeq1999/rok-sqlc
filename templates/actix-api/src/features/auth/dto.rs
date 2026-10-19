// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `auth` request and response bodies, named as the contract names
//! them.

use crate::db::types::Role;
use crate::db::users::models::{User, UserId};
use crate::shared::security::password::{MAX_LENGTH, MIN_LENGTH};

/// `POST /auth/register`'s and `POST /auth/login`'s body. The email is
/// trimmed and lowercased before it is checked.
#[derive(Debug, Clone, serde::Deserialize, validator::Validate, utoipa::ToSchema)]
pub struct CredentialsDto {
    /// The account's email address.
    #[serde(deserialize_with = "normalized_email")]
    #[validate(email(message = "email must be an email address"))]
    pub email: String,
    /// The password: 8 to 128 characters, anything allowed.
    #[validate(length(
        min = MIN_LENGTH,
        max = MAX_LENGTH,
        message = "password must be 8 to 128 characters"
    ))]
    pub password: String,
}

/// Body for redeeming a link token: email verification and email-change
/// confirmation.
#[derive(Debug, Clone, serde::Deserialize, validator::Validate, utoipa::ToSchema)]
pub struct VerifyEmailDto {
    /// The token the mailed link carries (`?token=`).
    #[validate(length(min = 1, message = "token must not be empty"))]
    pub token: String,
}

/// A session: a bearer token and how long it lasts.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct TokenResponse {
    /// The JWT to send as `Authorization: Bearer <access_token>`.
    pub access_token: String,
    /// Seconds until it expires.
    pub expires_in: i64,
    /// Always `Bearer`.
    pub token_type: String,
}

/// What `POST /auth/login` returns: either a real session (`TokenResponse`'s
/// shape) or, for a two-factor-enabled account, a marker plus the
/// short-lived token `POST /auth/2fa/verify` needs. Untagged, so each
/// variant serializes as its own plain object: a client tells them apart by
/// `access_token` vs `requires_totp`. This release only ever answers a
/// session; two-factor sign-in comes later.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
#[serde(untagged)]
pub enum LoginResponse {
    /// A session.
    Session(TokenResponse),
    /// A second factor is needed first.
    TotpRequired {
        /// Always `true`.
        requires_totp: bool,
        /// The token `POST /auth/2fa/verify` takes with the code.
        pending_token: String,
    },
}

/// A message for people, such as `email verified`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct MessageResponse {
    /// What happened.
    pub message: String,
}

impl MessageResponse {
    /// The response saying `message`.
    #[must_use]
    pub fn new(message: &str) -> Self {
        MessageResponse {
            message: message.to_owned(),
        }
    }
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

/// Deserializes an email address trimmed and lowercased, as it is stored
/// and compared.
fn normalized_email<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let email: String = serde::Deserialize::deserialize(deserializer)?;
    Ok(email.trim().to_lowercase())
}
