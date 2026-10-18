// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! [`AppError`]: every error a handler or service returns, answered with
//! the wire standard's body, `{"error": {"code", "message", "details"?}}`.
//! A new error case is a variant here, never an error type in a feature.

use actix_web::http::StatusCode;
use actix_web::{HttpResponse, ResponseError};
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
    /// `401 unauthorized`: no valid credentials.
    #[error("{0}")]
    Unauthorized(String),
    /// `403 forbidden`: not allowed.
    #[error("{0}")]
    Forbidden(String),
    /// `404 not_found`: no such row, or another caller's.
    #[error("{0}")]
    NotFound(String),
    /// `409 conflict`: a unique or foreign-key violation, naming the
    /// constraint.
    #[error("{0}")]
    Conflict(String),
    /// `413 payload_too_large`: a body over the limit.
    #[error("{0}")]
    PayloadTooLarge(String),
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
            AppError::Unauthorized(_) => "unauthorized",
            AppError::Forbidden(_) => "forbidden",
            AppError::NotFound(_) => "not_found",
            AppError::Conflict(_) => "conflict",
            AppError::PayloadTooLarge(_) => "payload_too_large",
            AppError::Validation { .. } => "validation_failed",
            AppError::PreconditionRequired(_) => "precondition_required",
            AppError::VersionConflict(_) => "version_conflict",
            AppError::Internal(_) => "internal_error",
        }
    }
}

impl ResponseError for AppError {
    fn status_code(&self) -> StatusCode {
        match self {
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::Forbidden(_) => StatusCode::FORBIDDEN,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            AppError::Validation { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            AppError::PreconditionRequired(_) => StatusCode::PRECONDITION_REQUIRED,
            AppError::VersionConflict(_) => StatusCode::PRECONDITION_FAILED,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        let (message, details) = match self {
            AppError::Internal(source) => {
                tracing::error!(error = %source, "internal error");
                (INTERNAL_MESSAGE.to_owned(), None)
            }
            AppError::Validation { message, details } => (message.clone(), details.clone()),
            other => (other.to_string(), None),
        };
        HttpResponse::build(self.status_code()).json(ErrorBody {
            error: ErrorPayload {
                code: self.code().to_owned(),
                message,
                details,
            },
        })
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
