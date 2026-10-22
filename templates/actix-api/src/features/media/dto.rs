// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `media` request and response bodies.

use crate::db::media::models::{Media, MediaId};
use crate::shared::pagination::Pagination;

/// The longest original file name an upload keeps, in characters.
pub const FILENAME_MAX: usize = 255;

/// `POST /media`'s query string.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct UploadMediaQuery {
    /// The file's original name, kept for display and as the download name.
    pub filename: Option<String>,
}

/// `GET /media`'s query string.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListMediaQuery {
    /// The page, from 1. Defaults to 1.
    pub page: Option<u32>,
    /// The files a page holds, 1 to 100. Defaults to 20.
    pub per_page: Option<u32>,
}

impl ListMediaQuery {
    /// The page asked for, with the defaults and limits applied.
    #[must_use]
    pub fn pagination(&self) -> Pagination {
        Pagination::new(self.page, self.per_page)
    }
}

/// A stored file's metadata, as the API answers it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct MediaResponse {
    /// The file's id.
    #[schema(value_type = uuid::Uuid)]
    pub id: MediaId,
    /// Detected from the file's bytes, not taken from the upload request.
    pub content_type: String,
    /// The file's size, in bytes.
    pub size_bytes: i64,
    /// The name it was uploaded with, if any.
    pub original_filename: Option<String>,
    /// Where to fetch the file. It needs the same credentials as any other endpoint.
    pub url: String,
    /// When it was uploaded.
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<Media> for MediaResponse {
    fn from(media: Media) -> Self {
        MediaResponse {
            url: content_url(media.id),
            id: media.id,
            content_type: media.content_type,
            size_bytes: media.size_bytes,
            original_filename: media.original_filename,
            created_at: media.created_at,
        }
    }
}

/// Where the file `id`'s bytes are served: `/api/v1/media/{id}/content`.
#[must_use]
pub fn content_url(id: MediaId) -> String {
    format!("/api/v1/media/{id}/content")
}

/// A file's bytes, as `GET /media/{id}/content` answers them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaContent {
    /// The bytes.
    pub bytes: bytes::Bytes,
    /// The type detected at upload.
    pub content_type: String,
    /// The `Content-Disposition` header: `inline` for an image, else
    /// `attachment`, with a safe download name.
    pub disposition: String,
}

impl MediaContent {
    /// `media`'s `bytes`, with its type and disposition.
    #[must_use]
    pub fn new(media: Media, bytes: bytes::Bytes) -> Self {
        let kind = if media.content_type.starts_with("image/") {
            "inline"
        } else {
            "attachment"
        };
        let disposition = format!(
            "{kind}; filename=\"{}\"",
            download_name(media.original_filename.as_deref())
        );
        MediaContent {
            bytes,
            content_type: media.content_type,
            disposition,
        }
    }
}

/// `name` reduced to characters safe inside a quoted header value
/// (`A-Z a-z 0-9 . - _`, anything else `_`), or `download`.
fn download_name(name: Option<&str>) -> String {
    let cleaned: String = name
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.trim_matches(['.', '_']).is_empty() {
        "download".to_owned()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_download_name_keeps_only_safe_characters() {
        assert_eq!(
            download_name(Some("holiday photo.png")),
            "holiday_photo.png"
        );
        assert_eq!(download_name(Some("a\"b\r\nc.pdf")), "a_b__c.pdf");
        assert_eq!(download_name(Some("../../etc/passwd")), ".._.._etc_passwd");
        assert_eq!(download_name(Some("...")), "download");
        assert_eq!(download_name(None), "download");
    }
}
