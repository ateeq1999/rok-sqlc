// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Raw request bodies (uploads): [`limited`] answers a body over its
//! route's limit with the wire standard's `413`.

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::http::StatusCode;

use crate::error::AppError;

/// The body a handler took as `Result<Bytes, BytesRejection>`. Over the
/// route's `DefaultBodyLimit`, axum's own answer is a plain-text `413`:
/// this answers [`AppError::too_large`] (the `what` over `limit` bytes)
/// instead. Any other rejection is a `400`.
pub fn limited(
    body: Result<Bytes, BytesRejection>,
    limit: usize,
    what: &str,
) -> Result<Bytes, AppError> {
    body.map_err(|rejection| {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            AppError::too_large(what, limit)
        } else {
            AppError::BadRequest(rejection.body_text())
        }
    })
}
