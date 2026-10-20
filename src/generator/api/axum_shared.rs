//! The shared tools `rok-sqlc g` regenerates in `shared/` that axum needs
//! of its own (0.7, decision A2): `health.rs`, `tenant.rs`, and the head of
//! `validation.rs`, which ends with the same `IdList` and `FieldFailures`
//! as actix-web's ([`SHARED_ITEMS`]).
//! `pagination.rs` and `response.rs` are the same for both frameworks.

use super::shared_validation::SHARED_ITEMS;

/// axum's `validation.rs`: [`VALIDATION_HEAD`], then the framework-free
/// [`SHARED_ITEMS`].
pub fn validation() -> String {
    format!("{VALIDATION_HEAD}{SHARED_ITEMS}")
}

pub(super) const HEALTH: &str = r#"//! `GET /health/live` and `GET /health/ready`, outside the API prefix.

use axum::extract::{FromRef, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use sqlx::PgPool;

use super::response::ErrorBody;
use crate::db::pool::check_health;

/// The `code` of `/health/ready`'s `503`.
pub const SERVICE_UNAVAILABLE: &str = "service_unavailable";

/// A healthy answer: `{"status": "ok"}` or `{"status": "ready"}`.
#[derive(Debug, Serialize)]
struct Status {
    status: &'static str,
}

/// `GET /health/live` and `GET /health/ready`. `/health/ready` reads the
/// pool from the state.
pub fn routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    PgPool: FromRef<S>,
{
    Router::new()
        .route("/health/live", get(liveness))
        .route("/health/ready", get(readiness))
}

/// `200 {"status": "ok"}` while the server answers at all.
pub async fn liveness() -> Response {
    Json(Status { status: "ok" }).into_response()
}

/// `200 {"status": "ready"}` when the database answers, otherwise `503`
/// with `SERVICE_UNAVAILABLE`.
pub async fn readiness(State(pool): State<PgPool>) -> Response {
    match check_health(&pool).await {
        Ok(()) => Json(Status { status: "ready" }).into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorBody::new(
                SERVICE_UNAVAILABLE,
                "the database is not reachable",
            )),
        )
            .into_response(),
    }
}
"#;

pub(super) const TENANT: &str = r#"//! [`Tenant`]: the tenant of a request, as the auth middleware found it.

use std::ops::Deref;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::error::AppError;

/// The tenant of a request, which the auth middleware stored in the
/// request's extensions (`request.extensions_mut().insert(tenant)`).
/// Handlers of tenant-scoped tables take one. A request without one is a
/// wiring bug, not the caller's fault: a `500` with `internal_error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tenant<T>(pub T);

impl<T> Tenant<T> {
    /// The tenant.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for Tenant<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T, S> FromRequestParts<S> for Tenant<T>
where
    T: Clone + Send + Sync + 'static,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, AppError> {
        let tenant = parts.extensions.get::<T>().cloned();
        tenant.map(Tenant).ok_or_else(|| {
            AppError::Internal(
                format!(
                    "no `{}` tenant in the request extensions: is the middleware that sets it mounted?",
                    std::any::type_name::<T>()
                )
                .into(),
            )
        })
    }
}
"#;

/// The head of axum's `validation.rs`: its imports, [`Path`] and [`Query`]
/// answering a malformed path or query with the wire standard's `400`,
/// and `IfMatch`.
const VALIDATION_HEAD: &str = r#"//! Reading a request: validated JSON bodies, the wire standard's `400` for a
//! malformed path or query, a versioned row's `If-Match`, and a
//! many-to-many sync's list of keys; and [`FieldFailures`], the failures of
//! a write's pre-checks.
//!
//! A handler takes [`ValidatedJson`], which checks the body's rules and
//! answers a bad body with a [`BodyRejection`] in the wire standard's error
//! body, and [`Path`] and [`Query`], axum's own answering a path or query
//! string that doesn't parse with a `400` saying why, worded the same
//! whichever framework serves the API.

use std::error::Error as _;
use std::str::FromStr;

use axum::extract::FromRequestParts;
use axum::extract::path::ErrorKind;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::http::request::Parts;
use axum::http::{HeaderMap, header};
use serde::de::DeserializeOwned;
use validator::{ValidationError, ValidationErrors};

use crate::error::AppError;

pub use crate::db::http::BodyRejection;
pub use crate::db::http::axum::ValidatedJson;

/// The path's parameters, as axum's `Path` reads them: a path that doesn't
/// parse, such as `/notes/not-a-uuid`, is a `400` with `bad_request`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Path<T>(pub T);

impl<T, S> FromRequestParts<S> for Path<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, AppError> {
        match axum::extract::Path::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Path(value)) => Ok(Path(value)),
            Err(rejection) => Err(AppError::BadRequest(path_message(&rejection))),
        }
    }
}

/// Why a path didn't parse: `Path deserialize error: ` and the value's
/// failure, such as `can not parse "abc" to a i64`.
fn path_message(rejection: &PathRejection) -> String {
    let PathRejection::FailedToDeserializePathParams(failed) = rejection else {
        return rejection.body_text();
    };
    let why = match failed.kind() {
        ErrorKind::Message(message) | ErrorKind::DeserializeError { message, .. } => {
            message.clone()
        }
        ErrorKind::ParseError {
            value,
            expected_type,
        }
        | ErrorKind::ParseErrorAtKey {
            value,
            expected_type,
            ..
        }
        | ErrorKind::ParseErrorAtIndex {
            value,
            expected_type,
            ..
        } => format!("can not parse {value:?} to a {expected_type}"),
        _ => return rejection.body_text(),
    };
    format!("Path deserialize error: {why}")
}

/// The query string, as axum's `Query` reads it: a query string that
/// doesn't parse, such as `?page=abc`, is a `400` with `bad_request`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Query<T>(pub T);

impl<T, S> FromRequestParts<S> for Query<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, AppError> {
        match axum::extract::Query::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Query(value)) => Ok(Query(value)),
            Err(rejection) => Err(AppError::BadRequest(query_message(&rejection))),
        }
    }
}

/// Why a query string didn't parse: `Query deserialize error: ` and the
/// failure, such as `invalid digit found in string`, without the field path
/// axum adds.
fn query_message(rejection: &QueryRejection) -> String {
    let mut source = rejection.source();
    while let Some(error) = source {
        type Failure = serde_path_to_error::Error<serde::de::value::Error>;
        if let Some(failure) = error.downcast_ref::<Failure>() {
            return format!("Query deserialize error: {}", failure.inner());
        }
        source = error.source();
    }
    rejection.body_text()
}

/// The version an `If-Match` header holds, `"<version>"`: the `ETag` a
/// versioned row was read with. A handler updating such a row takes it, so
/// a request without one is a `428` with `precondition_required`, and one
/// holding no version is a `412` with `version_conflict`, as it can't
/// match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IfMatch<T>(pub T);

impl<T> IfMatch<T> {
    /// The version.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T: FromStr, S: Send + Sync> FromRequestParts<S> for IfMatch<T> {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, AppError> {
        if_match(&parts.headers).map(IfMatch)
    }
}

/// The version `headers`' `If-Match` holds.
fn if_match<T: FromStr>(headers: &HeaderMap) -> Result<T, AppError> {
    let value = headers.get(header::IF_MATCH).ok_or_else(|| {
        AppError::PreconditionRequired("`If-Match` must hold the row's `ETag`".to_owned())
    })?;
    let version = value
        .to_str()
        .ok()
        .and_then(|value| value.trim().strip_prefix('"')?.strip_suffix('"'))
        .and_then(|version| version.parse().ok());
    version.ok_or_else(|| {
        AppError::VersionConflict("`If-Match` holds no version of the row".to_owned())
    })
}

"#;
