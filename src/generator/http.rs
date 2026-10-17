//! `db/http/`, generated with `extractors` (ADR 0015): `mod.rs`, the error
//! envelope and the rules every framework's extractor shares, and a module
//! per framework.

use std::path::Path;

use super::HEADER;
use crate::Extractor;

/// The `db/http/` files for `extractors`, under `output`; none without
/// extractors.
pub fn render(output: &Path, extractors: &[Extractor]) -> Vec<(std::path::PathBuf, String)> {
    if extractors.is_empty() {
        return Vec::new();
    }
    let dir = output.join("http");
    let mut files = vec![(dir.join("mod.rs"), render_mod(extractors))];
    if extractors.contains(&Extractor::Axum) {
        files.push((dir.join("axum.rs"), format!("{HEADER}\n{AXUM}")));
    }
    files
}

/// `db/http/mod.rs`: a module per framework, then the shared items.
fn render_mod(extractors: &[Extractor]) -> String {
    let mut output = format!("{HEADER}\n{MOD_DOC}\n");
    if extractors.contains(&Extractor::Axum) {
        output.push_str("pub mod axum;\n");
    }
    output.push('\n');
    output.push_str(SHARED);
    output
}

const MOD_DOC: &str = "//! Validated JSON bodies for HTTP handlers: the Rok error envelope, and the
//! rules that turn a request body into a validated body or a
//! [`BodyRejection`], shared by each framework's extractor.
";

/// What every framework's extractor shares: the envelope, the codes, the
/// rejection and its status and body, and the reading of a body.
const SHARED: &str = r#"use std::borrow::Cow;
use std::fmt;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::error::Category;
use validator::{Validate, ValidationError, ValidationErrors, ValidationErrorsKind};

/// The `code` of a body that is not JSON: bad syntax, a missing or wrong
/// `Content-Type`, or a body that could not be read.
pub const MALFORMED_BODY: &str = "MALFORMED_BODY";

/// The `code` of a JSON body that does not fit the type or breaks a
/// validation rule.
pub const VALIDATION_FAILED: &str = "VALIDATION_FAILED";

/// The message of a body breaking validation rules, which `details` lists.
const FAILED_RULES: &str = "the request body failed validation";

/// Where [`BodyRejection::unfit`] records serde's message: validator's key
/// for errors about the whole struct.
const WHOLE_BODY: &str = "__all__";

/// The code of the error [`BodyRejection::unfit`] records.
const UNFIT: &str = "deserialize";

/// The Rok error envelope: `{"success": false, "error": {...}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorBody {
    /// Always `false`.
    pub success: bool,
    /// What went wrong.
    pub error: ErrorPayload,
}

/// The `error` of an [`ErrorBody`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorPayload {
    /// A message for people.
    pub message: String,
    /// A code for programs: [`MALFORMED_BODY`] or [`VALIDATION_FAILED`].
    pub code: String,
    /// The broken validation rules by field, as validator serializes them,
    /// when there are any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<ValidationErrors>,
}

/// Why a request body is not a validated body. Each framework's module
/// answers it with [`BodyRejection::status`] and [`BodyRejection::body`].
#[derive(Debug, Clone, PartialEq)]
pub enum BodyRejection {
    /// The body is not JSON: bad syntax, a missing or wrong `Content-Type`,
    /// or a body that could not be read. Answered `400` with
    /// [`MALFORMED_BODY`].
    Malformed {
        /// What is wrong with the body.
        message: String,
    },
    /// The body is JSON but not a valid value of the type: it breaks
    /// validation rules, or it does not fit the type (a missing field, a
    /// wrong type, an unknown label). Answered `422` with
    /// [`VALIDATION_FAILED`]. A body that does not fit holds one error under
    /// `__all__` with the code `deserialize` and serde's message (see
    /// [`BodyRejection::unfit`]), answered with that message and no
    /// `details`.
    ValidationFailure(ValidationErrors),
}

impl BodyRejection {
    /// The rejection of a request whose `Content-Type` is not JSON.
    #[must_use]
    pub fn not_json() -> Self {
        BodyRejection::Malformed {
            message: "expected a request with `Content-Type: application/json`".to_owned(),
        }
    }

    /// The rejection of well-formed JSON that does not fit the type, with
    /// serde's `message`.
    #[must_use]
    pub fn unfit(message: impl Into<Cow<'static, str>>) -> Self {
        let mut errors = ValidationErrors::new();
        errors.add(
            WHOLE_BODY,
            ValidationError::new(UNFIT).with_message(message.into()),
        );
        BodyRejection::ValidationFailure(errors)
    }

    /// The HTTP status code answering the rejection: `400` for
    /// [`BodyRejection::Malformed`], `422` for
    /// [`BodyRejection::ValidationFailure`].
    #[must_use]
    pub fn status(&self) -> u16 {
        match self {
            BodyRejection::Malformed { .. } => 400,
            BodyRejection::ValidationFailure(_) => 422,
        }
    }

    /// The response body answering the rejection.
    #[must_use]
    pub fn body(&self) -> ErrorBody {
        let (message, code, details) = match self {
            BodyRejection::Malformed { message } => (message.clone(), MALFORMED_BODY, None),
            BodyRejection::ValidationFailure(errors) => match unfit_message(errors) {
                Some(message) => (message.to_owned(), VALIDATION_FAILED, None),
                None => (
                    FAILED_RULES.to_owned(),
                    VALIDATION_FAILED,
                    Some(errors.clone()),
                ),
            },
        };
        ErrorBody {
            success: false,
            error: ErrorPayload {
                message,
                code: code.to_owned(),
                details,
            },
        }
    }
}

impl fmt::Display for BodyRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.body().error.message)
    }
}

impl std::error::Error for BodyRejection {}

/// Serde's message, when `errors` is the one error of
/// [`BodyRejection::unfit`].
fn unfit_message(errors: &ValidationErrors) -> Option<&str> {
    if errors.errors().len() != 1 {
        return None;
    }
    let Some(ValidationErrorsKind::Field(errors)) = errors.errors().get(WHOLE_BODY) else {
        return None;
    };
    match errors.as_slice() {
        [error] if error.code == UNFIT => error.message.as_deref(),
        _ => None,
    }
}

/// Whether a `Content-Type` value names JSON: `application/json` or
/// `application/<anything>+json`, whatever its parameters.
#[must_use]
pub fn is_json(content_type: &str) -> bool {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    essence == "application/json"
        || (essence.starts_with("application/") && essence.ends_with("+json"))
}

/// Reads a JSON request body as a validated `T`.
///
/// # Errors
///
/// [`BodyRejection::Malformed`] when `bytes` is not JSON, and
/// [`BodyRejection::ValidationFailure`] when it does not fit `T` or breaks a
/// validation rule.
pub fn validated<T: DeserializeOwned + Validate>(bytes: &[u8]) -> Result<T, BodyRejection> {
    let body: T = serde_json::from_slice(bytes).map_err(|error| match error.classify() {
        Category::Data => BodyRejection::unfit(error.to_string()),
        Category::Io | Category::Syntax | Category::Eof => BodyRejection::Malformed {
            message: format!("the request body is not valid JSON: {error}"),
        },
    })?;
    body.validate().map_err(BodyRejection::ValidationFailure)?;
    Ok(body)
}
"#;

/// `db/http/axum.rs`.
const AXUM: &str = r"//! The axum extractor: [`ValidatedJson<T>`] takes a validated body, and
//! [`BodyRejection`] answers anything else in the Rok error envelope.

use std::ops::Deref;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use validator::Validate;

pub use super::BodyRejection;

/// A JSON request body that deserialized into `T` and passed every
/// validation rule. Any `T: DeserializeOwned + Validate` works: a generated
/// create input or update patch, or a hand-written request type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ValidatedJson<T>(pub T);

impl<T> ValidatedJson<T> {
    /// The validated body.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for ValidatedJson<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T, S> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = BodyRejection;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let is_json = request
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(super::is_json);
        if !is_json {
            return Err(BodyRejection::not_json());
        }
        let bytes = Bytes::from_request(request, state)
            .await
            .map_err(|rejection| BodyRejection::Malformed {
                message: rejection.body_text(),
            })?;
        super::validated(&bytes).map(Self)
    }
}

impl IntoResponse for BodyRejection {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status()).unwrap_or(StatusCode::BAD_REQUEST);
        (status, Json(self.body())).into_response()
    }
}
";
