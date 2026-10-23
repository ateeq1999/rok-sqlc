// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! `AccountService`: the signed-in user's own account. Reads go through
//! the data layer; the writes through this feature's query module,
//! `db::queries::account`. The profile photo's bytes go through the
//! `Storage` port (`shared/storage`), made by `shared/thumbnail`.

use std::sync::Arc;

use bytes::Bytes;
use sqlx::PgPool;

use super::dto::{AVATAR_MAX_BYTES, AVATAR_SIDE, UpdateProfileDto};
use crate::db::queries::account as queries;
use crate::db::users::models::{User, UserId};
use crate::db::users::read as users;
use crate::error::AppError;
use crate::shared::storage::Storage;
use crate::shared::thumbnail;

/// The prefix of every key a profile photo is stored under: the key is
/// `avatars/<file>`, and `users.avatar_url` is `/api/v1/avatars/<file>`.
const AVATAR_PREFIX: &str = "avatars/";

/// The `account` service, built by `app.rs`.
#[derive(Clone)]
pub struct AccountService {
    /// The pool the service reads and writes through.
    pub pool: PgPool,
    /// Where profile photos are.
    pub storage: Arc<dyn Storage>,
}

impl AccountService {
    /// The service over `pool`, its photos in `storage`.
    #[must_use]
    pub fn new(pool: PgPool, storage: Arc<dyn Storage>) -> Self {
        AccountService { pool, storage }
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

    /// Makes `image` (a JPEG, PNG or WebP image of at most 2 MiB, else a
    /// `413`) the account `id`'s photo: a 256×256 JPEG of its centre,
    /// re-encoded on a blocking thread (anything undecodable is a `422`),
    /// stored under a new random name. The previous photo is deleted.
    pub async fn set_avatar(&self, id: UserId, image: Bytes) -> Result<User, AppError> {
        if image.len() > AVATAR_MAX_BYTES {
            return Err(AppError::too_large("image", AVATAR_MAX_BYTES));
        }
        let jpeg = tokio::task::spawn_blocking(move || thumbnail::square_jpeg(&image, AVATAR_SIDE))
            .await
            .map_err(|error| AppError::Internal(error.into()))??;
        let key = format!("{AVATAR_PREFIX}{}", new_avatar_file());
        self.storage
            .put(&key, Bytes::from(jpeg), "image/jpeg")
            .await?;
        match self.swap_avatar(id, Some(&key)).await {
            Ok(user) => Ok(user),
            Err(error) => {
                self.forget(&key).await;
                Err(error)
            }
        }
    }

    /// Clears the account `id`'s photo, and deletes it.
    pub async fn remove_avatar(&self, id: UserId) -> Result<User, AppError> {
        self.swap_avatar(id, None).await
    }

    /// The JPEG bytes of the photo `file`. A name this service never
    /// generates, or a photo since replaced, is a `404`.
    pub async fn avatar(&self, file: &str) -> Result<Bytes, AppError> {
        if !is_avatar_file(file) {
            return Err(no_avatar());
        }
        let stored = self
            .storage
            .get(&format!("{AVATAR_PREFIX}{file}"))
            .await?
            .ok_or_else(no_avatar)?;
        Ok(stored.bytes)
    }

    /// Sets the account `id`'s avatar key to `key` (`None` clears it) in
    /// one transaction, then deletes the photo it replaced.
    async fn swap_avatar(&self, id: UserId, key: Option<&str>) -> Result<User, AppError> {
        let mut tx = self.pool.begin().await?;
        let previous = queries::lock_avatar_key(&mut *tx, id)
            .await?
            .ok_or_else(gone)?;
        let user = queries::set_avatar_key(&mut *tx, id, key)
            .await?
            .ok_or_else(gone)?;
        tx.commit().await?;
        if let Some(previous) = previous {
            self.forget(&previous).await;
        }
        Ok(user)
    }

    /// Deletes the photo under `key`, best effort: an orphaned file is
    /// harmless, so a failure is only logged.
    async fn forget(&self, key: &str) {
        if let Err(error) = self.storage.delete(key).await {
            tracing::warn!(%error, key, "could not delete a profile photo");
        }
    }
}

/// A new photo's file name: 128 random bits in lowercase hex, then `.jpg`.
/// Each upload gets a new name, which is what lets `GET /avatars/{file}`
/// be cached for good.
fn new_avatar_file() -> String {
    let bytes: [u8; 16] = rand::random();
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{hex}.jpg")
}

/// Whether `file` has the shape [`new_avatar_file`] gives: 32 lowercase
/// hex digits, then `.jpg`.
fn is_avatar_file(file: &str) -> bool {
    file.strip_suffix(".jpg").is_some_and(|name| {
        name.len() == 32
            && name
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// The `404` of an account deleted after its token was checked.
fn gone() -> AppError {
    AppError::NotFound("the account no longer exists".to_owned())
}

/// The `404` of a photo that doesn't exist.
fn no_avatar() -> AppError {
    AppError::NotFound("no such avatar".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_generated_file_names_are_avatars() {
        let file = new_avatar_file();
        assert!(is_avatar_file(&file), "{file}");
        assert_ne!(new_avatar_file(), file);
        for bad in [
            "",
            ".jpg",
            "../secret.jpg",
            "0123456789abcdef0123456789abcdef.png",
            "0123456789ABCDEF0123456789abcdef.jpg",
            "0123456789abcdef0123456789abcde/.jpg",
            "0123456789abcdef0123456789abcdef.jpg/x",
            "0123456789abcdef0123456789abcdef0.jpg",
        ] {
            assert!(!is_avatar_file(bad), "{bad}");
        }
    }
}
