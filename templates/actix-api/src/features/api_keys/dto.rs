// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `api_keys` request and response bodies.

use crate::db::api_keys::models::{ApiKey, ApiKeyId};
use crate::db::types::ApiKeyScope;

/// The longest key name, in characters.
pub const NAME_MAX: u64 = 100;

/// `POST /api-keys`'s body.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, validator::Validate, utoipa::ToSchema)]
pub struct CreateApiKeyDto {
    /// What the key is for, 1 to 100 characters after trimming.
    #[serde(deserialize_with = "trimmed")]
    #[validate(length(
        min = 1,
        max = NAME_MAX,
        message = "name must be 1 to 100 characters"
    ))]
    pub name: String,
    /// `read` (default): safe HTTP methods only. `write`: everything an API key may do.
    #[serde(default)]
    pub scope: Option<ApiKeyScope>,
    /// Omit for a key that never expires.
    #[serde(default)]
    #[validate(range(min = 1, max = 365, message = "expires_in_days must be 1 to 365"))]
    pub expires_in_days: Option<u32>,
}

/// An API key as the list answers it: never the key itself.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct ApiKeyResponse {
    /// The key's id.
    #[schema(value_type = uuid::Uuid)]
    pub id: ApiKeyId,
    /// What the key is for.
    pub name: String,
    /// First characters of the key, to recognise it in a list. The full key is never shown again.
    pub key_prefix: String,
    /// What the key may do.
    pub scope: ApiKeyScope,
    /// When the key stops working, if ever.
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    /// When the key was last used (to the minute).
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    /// When the key was made.
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<ApiKey> for ApiKeyResponse {
    fn from(row: ApiKey) -> Self {
        ApiKeyResponse {
            id: row.id,
            name: row.name,
            key_prefix: row.key_prefix,
            scope: row.scope,
            expires_at: row.expires_at,
            last_used_at: row.last_used_at,
            created_at: row.created_at,
        }
    }
}

/// Returned once, when the key is created: the only time the secret is visible.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct CreatedApiKeyResponse {
    /// The key as the list shows it.
    #[serde(flatten)]
    pub api_key: ApiKeyResponse,
    /// Send it as `Authorization: Bearer <key>` or `X-API-Key: <key>`. Store it now.
    pub key: String,
}

/// Deserializes a name, trimmed.
fn trimmed<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: String = serde::Deserialize::deserialize(deserializer)?;
    Ok(value.trim().to_owned())
}
