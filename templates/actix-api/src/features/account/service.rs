// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! `AccountService`: the signed-in user's own account. Reads go through
//! the data layer; the one write through this feature's query module,
//! `db::queries::account`.

use sqlx::PgPool;

use super::dto::UpdateProfileDto;
use crate::db::queries::account as queries;
use crate::db::users::models::{User, UserId};
use crate::db::users::read as users;
use crate::error::AppError;

/// The `account` service, registered as app data by `app.rs`.
pub struct AccountService {
    /// The pool the service reads and writes through.
    pub pool: PgPool,
}

impl AccountService {
    /// The service over `pool`.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        AccountService { pool }
    }

    /// The account `id`. `AuthUser` already checked it is live, so a
    /// `404` means it was deleted since.
    pub async fn me(&self, id: UserId) -> Result<User, AppError> {
        users::get_by_id(&self.pool, id).await?.ok_or_else(gone)
    }

    /// Changes the account `id`'s profile: the fields `input` holds.
    pub async fn update_profile(
        &self,
        id: UserId,
        input: UpdateProfileDto,
    ) -> Result<User, AppError> {
        let Some(display_name) = input.display_name else {
            return self.me(id).await;
        };
        queries::update_display_name(&self.pool, id, display_name.as_deref())
            .await?
            .ok_or_else(gone)
    }
}

/// The `404` of an account deleted after its token was checked.
fn gone() -> AppError {
    AppError::NotFound("the account no longer exists".to_owned())
}
