// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Raw request bodies (uploads): [`read`] reads one in the handler's body,
//! once its extractors, the caller's
//! [`AuthUser`](crate::shared::auth::AuthUser) among them, have succeeded.
//!
//! actix-web polls a handler's extractors together, so a `web::Bytes`
//! parameter starts buffering the body while `AuthUser` is still checking
//! the credential against the database: a caller who is then refused
//! could still make the server read up to the route's limit. A
//! `web::Payload` parameter reads nothing until [`read`] is called.

use actix_web::web;

use crate::error::AppError;

/// The body of `payload`, at most `limit` bytes. More is
/// [`AppError::too_large`] (the `what` over `limit` bytes), in the wire
/// standard's body rather than actix-web's plain text; a body that can't
/// be read is a `400`.
pub async fn read(payload: web::Payload, limit: usize, what: &str) -> Result<web::Bytes, AppError> {
    match payload.to_bytes_limited(limit).await {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(error)) => Err(AppError::BadRequest(error.to_string())),
        Err(_) => Err(AppError::too_large(what, limit)),
    }
}
