// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! [`AppError`]: every error a handler or service returns, answered with
//! the wire standard's body, `{"error": {"code", "message", "details"?}}`.
//! A new error case is a variant here, never an error type in a feature.

use axum::Json;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use sqlx::error::ErrorKind;
use validator::ValidationErrors;

use crate::shared::response::{ErrorBody, ErrorPayload};

/// The message of every `500`: the cause is logged, never sent.
const INTERNAL_MESSAGE: &str = "something went wrong";

/// An error answered with the wire standard's body. The message of each
/// variant but `Internal` is sent to the caller.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// `400 bad_request`: a malformed body, path or query.
    #[error("{0}")]
    BadRequest(String),
    /// `400 invalid_token`: a mailed link's token that is unknown, expired
    /// or already used.
    #[error("{0}")]
    InvalidToken(String),
    /// `400 wrong_password`: the current password a flow asks for again
    /// doesn't match.
    #[error("{0}")]
    WrongPassword(String),
    /// `400 breached_password`: a new password found in a known breach
    /// (`BREACH_CHECK`).
    #[error("{0}")]
    BreachedPassword(String),
    /// `401 unauthorized`: no valid credentials.
    #[error("{0}")]
    Unauthorized(String),
    /// `403 forbidden`: not allowed.
    #[error("{0}")]
    Forbidden(String),
    /// `404 not_found`: no such row, or another caller's.
    #[error("{0}")]
    NotFound(String),
    /// `408 request_timeout`: the request took longer than `app.rs`'
    /// timeout (`time_out`).
    #[error("{0}")]
    RequestTimeout(String),
    /// `409 conflict`: a unique or foreign-key violation, naming the
    /// constraint.
    #[error("{0}")]
    Conflict(String),
    /// `413 payload_too_large`: a body over the limit.
    #[error("{0}")]
    PayloadTooLarge(String),
    /// `415 unsupported_media_type`: an upload whose type, detected from its
    /// bytes, is unknown or not allowed.
    #[error("{0}")]
    UnsupportedMediaType(String),
    /// `422 validation_failed`: broken rules, with validator's `details`
    /// when there are any; also a check violation.
    #[error("{message}")]
    Validation {
        /// What is wrong.
        message: String,
        /// The broken rules by field.
        details: Option<ValidationErrors>,
    },
    /// `428 precondition_required`: `If-Match` missing on a versioned table.
    #[error("{0}")]
    PreconditionRequired(String),
    /// `412 version_conflict`: a stale `If-Match`.
    #[error("{0}")]
    VersionConflict(String),
    /// `429 too_many_requests`: over the rate limit. Answered with
    /// `Retry-After`, the seconds until the next request is allowed.
    #[error("too many requests; retry in {retry_after_secs}s")]
    TooManyRequests {
        /// Seconds until the caller may try again.
        retry_after_secs: u64,
    },
    /// `500 internal_error`: anything else. Logged with `tracing` when
    /// answered; the caller gets a generic message.
    #[error("internal error: {0}")]
    Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl AppError {
    /// The wire standard's `snake_case` code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            AppError::BadRequest(_) => "bad_request",
            AppError::InvalidToken(_) => "invalid_token",
            AppError::WrongPassword(_) => "wrong_password",
            AppError::BreachedPassword(_) => "breached_password",
            AppError::Unauthorized(_) => "unauthorized",
            AppError::Forbidden(_) => "forbidden",
            AppError::NotFound(_) => "not_found",
            AppError::RequestTimeout(_) => "request_timeout",
            AppError::Conflict(_) => "conflict",
            AppError::PayloadTooLarge(_) => "payload_too_large",
            AppError::UnsupportedMediaType(_) => "unsupported_media_type",
            AppError::Validation { .. } => "validation_failed",
            AppError::PreconditionRequired(_) => "precondition_required",
            AppError::VersionConflict(_) => "version_conflict",
            AppError::TooManyRequests { .. } => "too_many_requests",
            AppError::Internal(_) => "internal_error",
        }
    }

    /// The HTTP status answering the error.
    #[must_use]
    pub fn status_code(&self) -> StatusCode {
        match self {
            AppError::BadRequest(_)
            | AppError::InvalidToken(_)
            | AppError::WrongPassword(_)
            | AppError::BreachedPassword(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::Forbidden(_) => StatusCode::FORBIDDEN,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::RequestTimeout(_) => StatusCode::REQUEST_TIMEOUT,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            AppError::UnsupportedMediaType(_) => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            AppError::Validation { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            AppError::PreconditionRequired(_) => StatusCode::PRECONDITION_REQUIRED,
            AppError::VersionConflict(_) => StatusCode::PRECONDITION_FAILED,
            AppError::TooManyRequests { .. } => StatusCode::TOO_MANY_REQUESTS,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let code = self.code();
        let retry_after = match &self {
            AppError::TooManyRequests { retry_after_secs } => Some(*retry_after_secs),
            _ => None,
        };
        let (message, details) = match self {
            AppError::Internal(source) => {
                tracing::error!(error = %source, "internal error");
                (INTERNAL_MESSAGE.to_owned(), None)
            }
            AppError::Validation { message, details } => (message, details),
            other => (other.to_string(), None),
        };
        let body = Json(ErrorBody {
            error: ErrorPayload {
                code: code.to_owned(),
                message,
                details,
            },
        });
        match retry_after {
            Some(secs) => (status, [(header::RETRY_AFTER, secs.to_string())], body).into_response(),
            None => (status, body).into_response(),
        }
    }
}

/// Broken validation rules, as `dto.validate()?` returns them.
impl From<ValidationErrors> for AppError {
    fn from(details: ValidationErrors) -> Self {
        AppError::Validation {
            message: "the request failed validation".to_owned(),
            details: Some(details),
        }
    }
}

/// A unique or foreign-key violation is a `Conflict` naming the constraint,
/// a check violation is a `Validation`, `RowNotFound` is a `NotFound`, and
/// anything else is `Internal`.
impl From<sqlx::Error> for AppError {
    fn from(error: sqlx::Error) -> Self {
        if let sqlx::Error::RowNotFound = error {
            return AppError::NotFound("no such row".to_owned());
        }
        if let sqlx::Error::Database(database) = &error {
            let constraint = database.constraint().unwrap_or("unnamed");
            match database.kind() {
                ErrorKind::UniqueViolation => {
                    return AppError::Conflict(format!(
                        "the value is already taken (constraint `{constraint}`)"
                    ));
                }
                ErrorKind::ForeignKeyViolation => {
                    return AppError::Conflict(format!(
                        "the change breaks a reference (constraint `{constraint}`)"
                    ));
                }
                ErrorKind::CheckViolation => {
                    return AppError::Validation {
                        message: format!("the value breaks the check `{constraint}`"),
                        details: None,
                    };
                }
                _ => {}
            }
        }
        AppError::Internal(Box::new(error))
    }
}
