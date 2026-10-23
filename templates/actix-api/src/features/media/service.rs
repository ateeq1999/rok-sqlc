// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! `MediaService`: each caller's uploaded files. The bytes go through the
//! `Storage` port (`shared/storage`), the rows through this feature's
//! query module, `db::queries::media`. Every file is its owner's alone:
//! another caller's is a `404`, as if it didn't exist.

use std::sync::Arc;

use bytes::Bytes;
use sqlx::PgPool;

use super::dto::{FILENAME_MAX, MediaContent, MediaResponse};
use crate::db::media::models::MediaId;
use crate::db::queries::media as queries;
use crate::db::users::models::UserId;
use crate::error::AppError;
use crate::shared::pagination::Pagination;
use crate::shared::response::PaginatedResponse;
use crate::shared::storage::Storage;

/// The prefix of every key this feature stores under.
const KEY_PREFIX: &str = "media/";

/// The `media` service, built by `app.rs`.
#[derive(Clone)]
pub struct MediaService {
    /// The pool the rows are read and written through.
    pub pool: PgPool,
    /// Where the bytes are.
    pub storage: Arc<dyn Storage>,
    /// `MEDIA_MAX_UPLOAD_BYTES`: the largest upload. The routes enforce it
    /// on the body; [`MediaService::upload`] checks it again.
    pub max_upload_bytes: usize,
    /// `MEDIA_ALLOWED_TYPES`: the types, detected from the bytes, an upload
    /// may have.
    pub allowed_types: Vec<String>,
}

impl MediaService {
    /// Stores `bytes` for `owner`, named `filename`: the type is detected
    /// from the bytes (whatever the request claimed) and must be allowed
    /// (`415`); an empty body is a `400`, one over the limit a `413`, a
    /// name over 255 characters a `422`. The bytes are stored under a new
    /// key, then the row is written; when the row fails, the bytes are
    /// deleted again.
    pub async fn upload(
        &self,
        owner: UserId,
        bytes: Bytes,
        filename: Option<String>,
    ) -> Result<MediaResponse, AppError> {
        if bytes.is_empty() {
            return Err(AppError::BadRequest("the upload is empty".to_owned()));
        }
        if bytes.len() > self.max_upload_bytes {
            return Err(self.too_large());
        }
        let filename = filename.filter(|name| !name.trim().is_empty());
        if filename
            .as_deref()
            .is_some_and(|name| name.chars().count() > FILENAME_MAX)
        {
            return Err(AppError::Validation {
                message: format!("the file name must be at most {FILENAME_MAX} characters"),
                details: None,
            });
        }
        let Some(detected) = infer::get(&bytes) else {
            return Err(AppError::UnsupportedMediaType(
                "the file type could not be recognized".to_owned(),
            ));
        };
        let content_type = detected.mime_type();
        if !self
            .allowed_types
            .iter()
            .any(|allowed| allowed == content_type)
        {
            return Err(AppError::UnsupportedMediaType(format!(
                "files of type {content_type} are not accepted"
            )));
        }

        let id = MediaId::generate();
        let storage_key = format!("{KEY_PREFIX}{}", id.0.simple());
        let size_bytes = i64::try_from(bytes.len()).map_err(|_| self.too_large())?;
        self.storage.put(&storage_key, bytes, content_type).await?;
        let inserted = queries::insert_media(
            &self.pool,
            queries::InsertMediaParams {
                id,
                owner_id: owner,
                content_type: content_type.to_owned(),
                size_bytes,
                original_filename: filename,
                storage_key: storage_key.clone(),
            },
        )
        .await;
        match inserted {
            Ok(media) => Ok(media.into()),
            Err(error) => {
                self.forget(&storage_key).await;
                Err(error.into())
            }
        }
    }

    /// One page of `owner`'s files, newest first.
    pub async fn list(
        &self,
        owner: UserId,
        pagination: Pagination,
    ) -> Result<PaginatedResponse<MediaResponse>, AppError> {
        let total = queries::count_owned_media(&self.pool, owner).await?;
        let items =
            queries::list_owned_media(&self.pool, owner, pagination.limit(), pagination.offset())
                .await?;
        let items = items.into_iter().map(MediaResponse::from).collect();
        Ok(PaginatedResponse::new(items, pagination, total))
    }

    /// The file `id`'s metadata, when `owner` owns it (else `404`).
    pub async fn metadata(&self, owner: UserId, id: MediaId) -> Result<MediaResponse, AppError> {
        let media = queries::owned_media(&self.pool, id, owner)
            .await?
            .ok_or_else(not_found)?;
        Ok(media.into())
    }

    /// The file `id`'s bytes, when `owner` owns it (else `404`, also when
    /// its bytes are gone).
    pub async fn content(&self, owner: UserId, id: MediaId) -> Result<MediaContent, AppError> {
        let media = queries::owned_media(&self.pool, id, owner)
            .await?
            .ok_or_else(not_found)?;
        let Some(stored) = self.storage.get(&media.storage_key).await? else {
            tracing::warn!(media_id = %media.id, "a media row has no stored bytes");
            return Err(not_found());
        };
        Ok(MediaContent::new(media, stored.bytes))
    }

    /// Deletes the file `id`, when `owner` owns it (else `404`): the row,
    /// then its bytes (a failure there is logged, not answered).
    pub async fn delete(&self, owner: UserId, id: MediaId) -> Result<(), AppError> {
        let storage_key = queries::delete_owned_media(&self.pool, id, owner)
            .await?
            .ok_or_else(not_found)?;
        self.forget(&storage_key).await;
        Ok(())
    }

    /// Deletes the bytes under `key`, best effort: an orphaned object is
    /// harmless, so a failure is only logged.
    async fn forget(&self, key: &str) {
        if let Err(error) = self.storage.delete(key).await {
            tracing::warn!(%error, key, "could not delete stored bytes");
        }
    }

    /// The `413` for a body over [`MediaService::max_upload_bytes`].
    fn too_large(&self) -> AppError {
        AppError::too_large("file", self.max_upload_bytes)
    }
}

/// The `404` for a file the caller doesn't have.
fn not_found() -> AppError {
    AppError::NotFound("no such file".to_owned())
}
