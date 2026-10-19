//! The template of `shared/validation.rs`, one of the shared tools
//! [`super::shared`] renders: reading a request, and `FieldFailures`, which
//! collects the field failures of a write's database checks (`exists`,
//! `unique`) into one `422`.

pub(super) const VALIDATION: &str = r#"//! Reading a request: validated JSON bodies, the wire standard's `400` for a
//! malformed path or query, a versioned row's `If-Match`, and a
//! many-to-many sync's list of keys; and [`FieldFailures`], the failures of
//! a write's database checks.
//!
//! A handler takes [`ValidatedJson`], which checks the body's rules and
//! answers a bad body with a [`BodyRejection`] in the wire standard's error
//! body. Every feature's scope uses [`path_config`] and [`query_config`].

use std::future::{Ready, ready};
use std::str::FromStr;

use actix_web::dev::Payload;
use actix_web::http::header;
use actix_web::{FromRequest, HttpRequest, web};
use validator::{ValidationError, ValidationErrors};

use crate::error::AppError;

pub use crate::db::http::BodyRejection;
pub use crate::db::http::actix_web::ValidatedJson;

/// The `web::PathConfig` of every feature's scope: a path that doesn't
/// parse, such as `/notes/not-a-uuid`, is a `400` with `bad_request`.
#[must_use]
pub fn path_config() -> web::PathConfig {
    web::PathConfig::default()
        .error_handler(|error, _| AppError::BadRequest(error.to_string()).into())
}

/// The `web::QueryConfig` of every feature's scope: a query string that
/// doesn't parse, such as `?page=abc`, is a `400` with `bad_request`.
#[must_use]
pub fn query_config() -> web::QueryConfig {
    web::QueryConfig::default()
        .error_handler(|error, _| AppError::BadRequest(error.to_string()).into())
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

impl<T: FromStr> FromRequest for IfMatch<T> {
    type Error = AppError;
    type Future = Ready<Result<Self, AppError>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        ready(if_match(req).map(IfMatch))
    }
}

/// The version `req`'s `If-Match` holds.
fn if_match<T: FromStr>(req: &HttpRequest) -> Result<T, AppError> {
    let value = req.headers().get(header::IF_MATCH).ok_or_else(|| {
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

/// The body of a many-to-many sync, `{"ids": [...]}`: the keys of the
/// related rows to pair with, and no others.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, validator::Validate)]
pub struct IdList<T> {
    /// The related rows' keys.
    pub ids: Vec<T>,
}

/// The field failures a write's database checks find, such as a foreign
/// key naming no row the caller can see (`exists`) or a value another row
/// already has (`unique`). A generated `create_in`
/// or `update_in` runs every check on its transaction, adding each failure
/// here, then calls [`into_result`](Self::into_result) before it writes, so
/// one `422 validation_failed` lists them all, in the `details` shape of
/// the body's rules.
#[derive(Debug, Default)]
pub struct FieldFailures(ValidationErrors);

impl FieldFailures {
    /// Adds the failure `code` of `field`, with `message`.
    pub fn add(&mut self, field: &'static str, code: &'static str, message: String) {
        let error = ValidationError::new(code).with_message(message.into());
        self.0.add(field, error);
    }

    /// Adds the `exists` failure of `field`, which names no `singular` row
    /// the caller can see.
    pub fn missing(&mut self, field: &'static str, singular: &str) {
        let message = format!("{field} does not refer to an existing {singular}");
        self.add(field, "exists", message);
    }

    /// Adds the `unique` failure of `field`, whose value another row
    /// already has.
    pub fn taken(&mut self, field: &'static str) {
        self.add(field, "unique", format!("{field} is already taken"));
    }

    /// Adds the `unique` failure of a composite key, whose values together
    /// another row already has, against its last field.
    pub fn taken_together(&mut self, fields: &[&'static str]) {
        if let [init @ .., last] = fields {
            let names = if init.is_empty() {
                (*last).to_owned()
            } else {
                format!("{} and {last}", init.join(", "))
            };
            let message = format!("{names} are already taken together");
            self.add(last, "unique", message);
        }
    }

    /// Whether no failure was added.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `Ok` when no failure was added.
    ///
    /// # Errors
    ///
    /// The `422 validation_failed` listing every failure added.
    pub fn into_result(self) -> Result<(), AppError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(AppError::from(self.0))
        }
    }
}
"#;
