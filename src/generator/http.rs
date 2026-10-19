//! `db/http/`, generated with `extractors` (ADR 0015): `mod.rs`, the wire
//! standard's error body (ADR 0016) and the rules every framework's extractor
//! shares, and a module per framework.

use std::path::Path;

use super::{DeriveOptions, HEADER, replace_once};
use crate::Extractor;

/// The module `extractors` generates, and so reserves while they are set.
pub const MODULE: &str = "http";

/// The `db/http/` files for `extractors`, under `output`; none without
/// extractors. With `derives.openapi`, the error body derives `ToSchema`.
pub fn render(
    output: &Path,
    extractors: &[Extractor],
    derives: DeriveOptions,
) -> Vec<(std::path::PathBuf, String)> {
    if extractors.is_empty() {
        return Vec::new();
    }
    let dir = output.join(MODULE);
    let mut files = vec![(dir.join("mod.rs"), render_mod(extractors, derives))];
    if extractors.contains(&Extractor::ActixWeb) {
        files.push((dir.join("actix_web.rs"), format!("{HEADER}\n{ACTIX_WEB}")));
    }
    if extractors.contains(&Extractor::Axum) {
        files.push((dir.join("axum.rs"), format!("{HEADER}\n{AXUM}")));
    }
    files
}

/// `db/http/mod.rs`: a module per framework, then the shared items.
fn render_mod(extractors: &[Extractor], derives: DeriveOptions) -> String {
    let mut output = format!("{HEADER}\n{MOD_DOC}\n");
    // Sorted the way rustfmt sorts `mod` declarations.
    if extractors.contains(&Extractor::ActixWeb) {
        output.push_str("pub mod actix_web;\n");
    }
    if extractors.contains(&Extractor::Axum) {
        output.push_str("pub mod axum;\n");
    }
    output.push('\n');
    if derives.openapi {
        output.push_str(&described(SHARED));
    } else {
        output.push_str(SHARED);
    }
    output
}

/// `shared` with the error body's two types deriving `ToSchema`, and
/// validator's `details` described as a free-form object.
fn described(shared: &str) -> String {
    let mut text = shared.to_owned();
    for item in ["pub struct ErrorBody {", "pub struct ErrorPayload {"] {
        text = replace_once(
            &text,
            &format!("Serialize, Deserialize)]\n{item}"),
            &format!("Serialize, Deserialize, utoipa::ToSchema)]\n{item}"),
        );
    }
    replace_once(
        &text,
        "    pub details: Option<ValidationErrors>,",
        "    #[schema(value_type = Option<Object>)]\n    pub details: Option<ValidationErrors>,",
    )
}

const MOD_DOC: &str = "//! Validated bodies for HTTP handlers: the wire standard's error body, and
//! the rules that turn a framework's JSON body, or its rejection, into a
//! validated body or a [`BodyRejection`], shared by each framework's
//! extractor.
";

/// What every framework's extractor shares: the error body, the codes, the
/// rejection and its status and body, and the checking of a body.
const SHARED: &str = r#"use std::borrow::Cow;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::error::Category;
use validator::{Validate, ValidationError, ValidationErrors, ValidationErrorsKind};

/// The `code` of a body that is not JSON: bad syntax, a missing or wrong
/// `Content-Type`, or a body that could not be read.
pub const BAD_REQUEST: &str = "bad_request";

/// The `code` of a body over the framework's size limit.
pub const PAYLOAD_TOO_LARGE: &str = "payload_too_large";

/// The `code` of a JSON body that does not fit the type or breaks a
/// validation rule.
pub const VALIDATION_FAILED: &str = "validation_failed";

/// The message of a body breaking validation rules, which `details` lists.
const FAILED_RULES: &str = "the request body failed validation";

/// The message of a body over the framework's size limit.
const OVER_LIMIT: &str = "the request body is over the size limit";

/// Where [`BodyRejection::unfit`] records serde's message: validator's key
/// for errors about the whole struct.
const WHOLE_BODY: &str = "__all__";

/// The code of the error [`BodyRejection::unfit`] records.
const UNFIT: &str = "deserialize";

/// The wire standard's error body: `{"error": {"code", "message",
/// "details"?}}`. Any error answer can use it, not only a rejected body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorBody {
    /// What went wrong.
    pub error: ErrorPayload,
}

impl ErrorBody {
    /// The body answering a `snake_case` `code`, such as [`BAD_REQUEST`], with
    /// `message` and no `details`.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        ErrorBody {
            error: ErrorPayload {
                code: code.into(),
                message: message.into(),
                details: None,
            },
        }
    }
}

/// The `error` of an `ErrorBody`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorPayload {
    /// A `snake_case` code for programs, such as `BAD_REQUEST`,
    /// `PAYLOAD_TOO_LARGE` or `VALIDATION_FAILED`.
    pub code: String,
    /// A message for people.
    pub message: String,
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
    /// [`BAD_REQUEST`].
    Malformed {
        /// What is wrong with the body.
        message: String,
    },
    /// The body is over the framework's size limit (axum's
    /// `DefaultBodyLimit`, actix-web's `JsonConfig`). Answered `413` with
    /// [`PAYLOAD_TOO_LARGE`].
    TooLarge,
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

    /// The rejection of a body that could not be read, other than one over
    /// the size limit.
    #[must_use]
    pub fn unreadable() -> Self {
        BodyRejection::Malformed {
            message: "the request body could not be read".to_owned(),
        }
    }

    /// The rejection of a body serde could not parse as the type: bad
    /// syntax, or well-formed JSON that does not fit the type, with serde's
    /// own message, which both frameworks give alike.
    #[must_use]
    pub fn from_serde(error: &serde_json::Error) -> Self {
        match error.classify() {
            Category::Data => BodyRejection::unfit(error.to_string()),
            Category::Io | Category::Syntax | Category::Eof => BodyRejection::Malformed {
                message: format!("the request body is not valid JSON: {error}"),
            },
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
    /// [`BodyRejection::Malformed`], `413` for [`BodyRejection::TooLarge`],
    /// `422` for [`BodyRejection::ValidationFailure`].
    #[must_use]
    pub fn status(&self) -> u16 {
        match self {
            BodyRejection::Malformed { .. } => 400,
            BodyRejection::TooLarge => 413,
            BodyRejection::ValidationFailure(_) => 422,
        }
    }

    /// The response body answering the rejection.
    #[must_use]
    pub fn body(&self) -> ErrorBody {
        let (message, code, details) = match self {
            BodyRejection::Malformed { message } => (message.clone(), BAD_REQUEST, None),
            BodyRejection::TooLarge => (OVER_LIMIT.to_owned(), PAYLOAD_TOO_LARGE, None),
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
            error: ErrorPayload {
                code: code.to_owned(),
                message,
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

/// Checks a deserialized body against its validation rules.
///
/// # Errors
///
/// [`BodyRejection::ValidationFailure`] when `body` breaks a validation
/// rule.
pub fn validated<T: Validate>(body: T) -> Result<T, BodyRejection> {
    body.validate().map_err(BodyRejection::ValidationFailure)?;
    Ok(body)
}
"#;

/// `db/http/axum.rs`.
const AXUM: &str = r"//! The axum extractor: [`ValidatedJson<T>`] takes a validated body, and
//! [`BodyRejection`] answers anything else with the wire standard's error
//! body.
//!
//! The body goes through `axum::Json<T>`, so the router's `DefaultBodyLimit`
//! applies (2 MiB unless set). A body over it is answered `413`.

use std::error::Error;
use std::ops::Deref;

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
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
        let Json(body) = Json::<T>::from_request(request, state)
            .await
            .map_err(|rejection| rejected(&rejection))?;
        super::validated(body).map(Self)
    }
}

/// The [`BodyRejection`] answering axum's own rejection.
fn rejected(rejection: &JsonRejection) -> BodyRejection {
    match rejection {
        JsonRejection::MissingJsonContentType(_) => BodyRejection::not_json(),
        JsonRejection::BytesRejection(rejection)
            if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE =>
        {
            BodyRejection::TooLarge
        }
        JsonRejection::JsonDataError(_) | JsonRejection::JsonSyntaxError(_) => {
            match serde_error(rejection) {
                Some(error) => BodyRejection::from_serde(error),
                None => BodyRejection::Malformed {
                    message: rejection.body_text(),
                },
            }
        }
        _ => BodyRejection::unreadable(),
    }
}

/// serde's own error inside axum's rejection. axum wraps a parse error in
/// `serde_path_to_error`'s, whose message adds the field path, so the
/// message comes from serde's error inside it, as actix-web gives it.
fn serde_error(rejection: &JsonRejection) -> Option<&serde_json::Error> {
    let mut source = rejection.source();
    while let Some(error) = source {
        if let Some(error) = error.downcast_ref::<serde_json::Error>() {
            return Some(error);
        }
        if let Some(error) = error.downcast_ref::<serde_path_to_error::Error<serde_json::Error>>() {
            return Some(error.inner());
        }
        source = error.source();
    }
    None
}

impl IntoResponse for BodyRejection {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status()).unwrap_or(StatusCode::BAD_REQUEST);
        (status, Json(self.body())).into_response()
    }
}
";

/// `db/http/actix_web.rs`.
const ACTIX_WEB: &str = r"//! The actix-web extractor: [`ValidatedJson<T>`] takes a validated body,
//! and [`BodyRejection`] answers anything else with the wire standard's
//! error body, so no `JsonConfig` error handler is needed.
//!
//! The body goes through `web::Json<T>`, so the app's `JsonConfig` applies:
//! its limit (2 MiB unless set; a body over it is answered `413`) and its
//! `Content-Type` predicate. Leave its error handler unset: an error it
//! returns is answered as a malformed body with that error's message.

use std::future::Future;
use std::ops::Deref;
use std::pin::Pin;

use actix_web::dev::Payload;
use actix_web::error::JsonPayloadError;
use actix_web::http::StatusCode;
use actix_web::{FromRequest, HttpRequest, HttpResponse, ResponseError, web};
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

impl<T> FromRequest for ValidatedJson<T>
where
    T: DeserializeOwned + Validate + 'static,
{
    type Error = BodyRejection;
    type Future = Pin<Box<dyn Future<Output = Result<Self, BodyRejection>>>>;

    fn from_request(request: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let body = web::Json::<T>::from_request(request, payload);
        Box::pin(async move {
            let web::Json(body) = body.await.map_err(|error| rejected(&error))?;
            super::validated(body).map(Self)
        })
    }
}

/// The [`BodyRejection`] answering actix-web's own error.
fn rejected(error: &actix_web::Error) -> BodyRejection {
    let Some(error) = error.as_error::<JsonPayloadError>() else {
        // What a `JsonConfig` error handler returned.
        return BodyRejection::Malformed {
            message: error.to_string(),
        };
    };
    match error {
        JsonPayloadError::OverflowKnownLength { .. } | JsonPayloadError::Overflow { .. } => {
            BodyRejection::TooLarge
        }
        JsonPayloadError::Payload(error)
            if error.status_code() == StatusCode::PAYLOAD_TOO_LARGE =>
        {
            BodyRejection::TooLarge
        }
        JsonPayloadError::ContentType => BodyRejection::not_json(),
        JsonPayloadError::Deserialize(error) => BodyRejection::from_serde(error),
        _ => BodyRejection::unreadable(),
    }
}

impl ResponseError for BodyRejection {
    fn status_code(&self) -> StatusCode {
        StatusCode::from_u16(self.status()).unwrap_or(StatusCode::BAD_REQUEST)
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(self.body())
    }
}
";

#[cfg(test)]
mod tests {
    use super::*;

    /// `SHARED` still holds what [`described`] patches: a patch that misses
    /// panics in a debug build.
    #[test]
    fn the_error_body_is_described() {
        let text = described(SHARED);
        for item in ["pub struct ErrorBody {", "pub struct ErrorPayload {"] {
            let derived = format!("Deserialize, utoipa::ToSchema)]\n{item}");
            assert!(text.contains(&derived), "{text}");
        }
        assert!(text.contains("#[schema(value_type = Option<Object>)]\n    pub details"));
    }
}
