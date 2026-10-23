// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Raw request bodies (uploads): [`limited`] answers a body over its
//! route's limit with the wire standard's `413`.

use actix_web::http::StatusCode;
use actix_web::web;

use crate::error::AppError;

/// The body a handler took as `Result<web::Bytes, actix_web::Error>`. Over
/// the route's `PayloadConfig` limit, actix-web's own answer is a
/// plain-text `413`: this answers [`AppError::too_large`] (the `what`
/// over `limit` bytes) instead. Any other failure is a `400`.
pub fn limited(
    body: Result<web::Bytes, actix_web::Error>,
    limit: usize,
    what: &str,
) -> Result<web::Bytes, AppError> {
    body.map_err(|error| {
        if error.as_response_error().status_code() == StatusCode::PAYLOAD_TOO_LARGE {
            AppError::too_large(what, limit)
        } else {
            AppError::BadRequest(error.to_string())
        }
    })
}
