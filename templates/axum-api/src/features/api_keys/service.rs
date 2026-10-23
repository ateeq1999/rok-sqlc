// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! `ApiKeyService`: a user's own API keys, through this feature's query
//! module, `db::queries::api_keys`. Checking a presented key is
//! `shared/auth.rs`'s (with `shared::security::api_key`).

use chrono::{TimeDelta, Utc};
use sqlx::PgPool;

use super::dto::{ApiKeyResponse, CreateApiKeyDto, CreatedApiKeyResponse};
use crate::db::api_keys::models::ApiKeyId;
use crate::db::queries::api_keys::{self as queries, InsertApiKeyParams};
use crate::db::types::ApiKeyScope;
use crate::db::users::models::UserId;
use crate::error::AppError;
use crate::shared::security::api_key::{MAX_ACTIVE, NewApiKey};

/// The `api_keys` service, built by `app.rs`.
pub struct ApiKeyService {
    /// The pool the service reads and writes through.
    pub pool: PgPool,
}

impl ApiKeyService {
    /// The service over `pool`.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        ApiKeyService { pool }
    }

    /// The user `owner`'s active keys (not revoked, not expired), newest
    /// first.
    pub async fn list(&self, owner: UserId) -> Result<Vec<ApiKeyResponse>, AppError> {
        let rows = queries::active_api_keys(&self.pool, owner).await?;
        Ok(rows.into_iter().map(ApiKeyResponse::from).collect())
    }

    /// A new key for the user `owner`, answered with the raw key this once.
    /// A user with [`MAX_ACTIVE`] active keys already is a `409 conflict`:
    /// they revoke one first. The owner's row is locked while counting, so
    /// two requests at once can't both make the last one.
    pub async fn create(
        &self,
        owner: UserId,
        input: CreateApiKeyDto,
    ) -> Result<CreatedApiKeyResponse, AppError> {
        let mut tx = self.pool.begin().await?;
        queries::lock_key_owner(&mut *tx, owner)
            .await?
            .ok_or_else(|| AppError::NotFound("the account no longer exists".to_owned()))?;
        if queries::count_active_api_keys(&mut *tx, owner).await? >= MAX_ACTIVE {
            return Err(AppError::Conflict(format!(
                "you can have at most {MAX_ACTIVE} active API keys; revoke one first"
            )));
        }
        let key = NewApiKey::generate();
        let row = queries::insert_api_key(
            &mut *tx,
            InsertApiKeyParams {
                id: ApiKeyId::generate(),
                user_id: owner,
                name: input.name,
                key_prefix: key.prefix,
                key_hash: key.hash,
                scope: input.scope.unwrap_or(ApiKeyScope::Read),
                expires_at: input
                    .expires_in_days
                    .map(|days| Utc::now() + TimeDelta::days(i64::from(days))),
            },
        )
        .await?;
        tx.commit().await?;
        Ok(CreatedApiKeyResponse {
            api_key: row.into(),
            key: key.raw,
        })
    }

    /// Revokes the user `owner`'s key `id`. Another user's key, an unknown
    /// one or one already revoked is a `404`.
    pub async fn revoke(&self, owner: UserId, id: ApiKeyId) -> Result<(), AppError> {
        match queries::revoke_api_key(&self.pool, id, owner).await? {
            0 => Err(AppError::NotFound("no such API key".to_owned())),
            _ => Ok(()),
        }
    }
}
