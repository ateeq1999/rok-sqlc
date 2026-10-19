//! The infrastructure `api.output` generates beside the API features (ADR
//! 0016): `db/pool.rs` in the data layer, and the regenerated tools in
//! `shared/`, the directory beside the features directory. `shared/mod.rs`
//! is scaffolded by `rok-sqlc init` and declares them; `shared/` holds the
//! user's own tools too, so each tool is a file of its own.
//!
//! The templates name the data layer `crate::db`; [`render`] rewrites that
//! to `api.db_path`.

use std::path::{Path, PathBuf};

use super::Feature;
use crate::ApiConfig;
use crate::generator::HEADER;

pub(super) use super::shared_validation::VALIDATION;

/// The data-layer path the templates are written against.
const TEMPLATE_DB_PATH: &str = "crate::db::";

/// The directory the shared tools go in: `shared/` beside `api.output`.
pub fn shared_dir(api: &ApiConfig) -> PathBuf {
    api.output
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join("shared")
}

/// `db/pool.rs` under the data layer's `output`, then each shared tool,
/// described for the OpenAPI document with `api.openapi`. `validation.rs`
/// also holds the helpers `features`' string rules call.
pub fn render(output: &Path, api: &ApiConfig, features: &[Feature<'_>]) -> Vec<(PathBuf, String)> {
    let db_path = format!("{}::", api.db_path);
    let file =
        |template: &str| format!("{HEADER}\n{}", template.replace(TEMPLATE_DB_PATH, &db_path));
    let shared = shared_dir(api);
    let mut files = vec![(output.join("pool.rs"), file(POOL))];
    for (name, template) in [
        ("health.rs", HEALTH),
        ("pagination.rs", PAGINATION),
        ("response.rs", RESPONSE),
        ("tenant.rs", TENANT),
        ("validation.rs", VALIDATION),
    ] {
        let text = file(template);
        let mut text = if api.openapi {
            super::schemas::describe(name, &text)
        } else {
            text
        };
        if name == "validation.rs" {
            text.push_str(&super::rule_functions::shared_helpers(features));
        }
        files.push((shared.join(name), text));
    }
    files
}

const POOL: &str = r#"//! The database pool: its settings, opening it, and checking that the
//! database answers. It reads no environment variables; `config.rs` fills
//! [`PoolSettings`].

use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// The pool's size and timeouts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolSettings {
    /// The most connections open at once.
    pub max_connections: u32,
    /// The connections kept open while idle.
    pub min_connections: u32,
    /// How long a caller waits for a connection before failing.
    pub acquire_timeout: Duration,
    /// How long a connection may sit idle before it is closed.
    pub idle_timeout: Duration,
    /// How long a connection lives before it is replaced.
    pub max_lifetime: Duration,
}

impl Default for PoolSettings {
    /// At most 20 connections, 5 kept open, 3 seconds to get one, closed
    /// after 10 idle minutes, replaced after 30.
    fn default() -> Self {
        PoolSettings {
            max_connections: 20,
            min_connections: 5,
            acquire_timeout: Duration::from_secs(3),
            idle_timeout: Duration::from_secs(600),
            max_lifetime: Duration::from_secs(1800),
        }
    }
}

impl PoolSettings {
    /// The default settings sized for a database server with `cpu_cores`
    /// cores and `spindles` disks: at most `cpu_cores * 2 + spindles`
    /// connections (`PostgreSQL`'s rule of thumb, and at least one), and no
    /// more kept open than that.
    #[must_use]
    pub fn for_hardware(cpu_cores: u32, spindles: u32) -> Self {
        let defaults = PoolSettings::default();
        let max_connections = cpu_cores.saturating_mul(2).saturating_add(spindles).max(1);
        PoolSettings {
            max_connections,
            min_connections: defaults.min_connections.min(max_connections),
            ..defaults
        }
    }
}

/// Opens a pool on the `PostgreSQL` `url` with `settings`, connecting before
/// it returns.
///
/// # Errors
///
/// The error of connecting, also logged.
pub async fn init_pool(url: &str, settings: &PoolSettings) -> Result<PgPool, sqlx::Error> {
    tracing::info!(
        max_connections = settings.max_connections,
        min_connections = settings.min_connections,
        "opening the database pool"
    );
    PgPoolOptions::new()
        .max_connections(settings.max_connections)
        .min_connections(settings.min_connections)
        .acquire_timeout(settings.acquire_timeout)
        .idle_timeout(settings.idle_timeout)
        .max_lifetime(settings.max_lifetime)
        .connect(url)
        .await
        .inspect_err(|error| tracing::error!(%error, "could not open the database pool"))
}

/// Checks that the database answers, running `SELECT 1` on a pooled
/// connection.
///
/// # Errors
///
/// The error of getting a connection or running the query, also logged.
pub async fn check_health(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1")
        .execute(pool)
        .await
        .map(|_| ())
        .inspect_err(|error| tracing::warn!(%error, "the database health check failed"))
}
"#;

pub(super) const HEALTH: &str = r#"//! `GET /health/live` and `GET /health/ready`, outside the API prefix.

use actix_web::{HttpResponse, web};
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

/// Mounts `GET /health/live` and `GET /health/ready`. `/health/ready` reads
/// the pool from the `web::Data<PgPool>` app data.
pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/health/live", web::get().to(liveness))
        .route("/health/ready", web::get().to(readiness));
}

/// `200 {"status": "ok"}` while the server answers at all.
pub async fn liveness() -> HttpResponse {
    HttpResponse::Ok().json(Status { status: "ok" })
}

/// `200 {"status": "ready"}` when the database answers, otherwise `503`
/// with `SERVICE_UNAVAILABLE`.
pub async fn readiness(pool: web::Data<PgPool>) -> HttpResponse {
    match check_health(&pool).await {
        Ok(()) => HttpResponse::Ok().json(Status { status: "ready" }),
        Err(_) => HttpResponse::ServiceUnavailable().json(ErrorBody::new(
            SERVICE_UNAVAILABLE,
            "the database is not reachable",
        )),
    }
}
"#;

pub(super) const PAGINATION: &str = r#"//! A list endpoint's query, `?page=&per_page=&q=&sort=&order=` (`q` only
//! where the table has search columns), and the page it asks for.

use serde::{Deserialize, Serialize};

use crate::db::search::{Direction, Page};

/// The direction of a list's sort: `order=asc` or `order=desc`, the
/// default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    /// Smallest first.
    Asc,
    /// Largest first.
    #[default]
    Desc,
}

impl From<SortOrder> for Direction {
    fn from(order: SortOrder) -> Self {
        match order {
            SortOrder::Asc => Direction::Asc,
            SortOrder::Desc => Direction::Desc,
        }
    }
}

/// A list endpoint's query string. `S` is what `sort` takes: a table's
/// generated sort key, or any text by default.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ListQuery<S = String> {
    /// The page, from 1. Defaults to 1.
    pub page: Option<u32>,
    /// The rows a page holds, 1 to 100. Defaults to 20.
    pub per_page: Option<u32>,
    /// The text to search for.
    pub q: Option<String>,
    /// What to sort by.
    pub sort: Option<S>,
    /// The sort's direction. Defaults to `desc`.
    pub order: Option<SortOrder>,
}

impl<S> Default for ListQuery<S> {
    fn default() -> Self {
        ListQuery {
            page: None,
            per_page: None,
            q: None,
            sort: None,
            order: None,
        }
    }
}

impl<S> ListQuery<S> {
    /// The page asked for, with the defaults and limits applied.
    #[must_use]
    pub fn pagination(&self) -> Pagination {
        Pagination::new(self.page, self.per_page)
    }

    /// The sort's direction, `desc` unless `order` says otherwise.
    #[must_use]
    pub fn sort_order(&self) -> SortOrder {
        self.order.unwrap_or_default()
    }
}

/// The query string of a list whose table has no search columns: a
/// `ListQuery` without `q`. `S` is what `sort` takes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PageQuery<S = String> {
    /// The page, from 1. Defaults to 1.
    pub page: Option<u32>,
    /// The rows a page holds, 1 to 100. Defaults to 20.
    pub per_page: Option<u32>,
    /// What to sort by.
    pub sort: Option<S>,
    /// The sort's direction. Defaults to `desc`.
    pub order: Option<SortOrder>,
}

impl<S> Default for PageQuery<S> {
    fn default() -> Self {
        PageQuery {
            page: None,
            per_page: None,
            sort: None,
            order: None,
        }
    }
}

impl<S> PageQuery<S> {
    /// The page asked for, with the defaults and limits applied.
    #[must_use]
    pub fn pagination(&self) -> Pagination {
        Pagination::new(self.page, self.per_page)
    }

    /// The sort's direction, `desc` unless `order` says otherwise.
    #[must_use]
    pub fn sort_order(&self) -> SortOrder {
        self.order.unwrap_or_default()
    }
}

/// A page of a list: its number, from 1, and the rows it holds, 1 to
/// [`Pagination::MAX_PER_PAGE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pagination {
    /// The page, from 1.
    pub page: u32,
    /// The rows a page holds.
    pub per_page: u32,
}

impl Pagination {
    /// The rows a page holds when `per_page` is not given.
    pub const DEFAULT_PER_PAGE: u32 = 20;
    /// The most rows a page holds.
    pub const MAX_PER_PAGE: u32 = 100;

    /// The page asked for: `page` defaults to 1 (and 0 is 1), `per_page`
    /// to [`Pagination::DEFAULT_PER_PAGE`], clamped to 1 to
    /// [`Pagination::MAX_PER_PAGE`].
    #[must_use]
    pub fn new(page: Option<u32>, per_page: Option<u32>) -> Self {
        Pagination {
            page: page.unwrap_or(1).max(1),
            per_page: per_page
                .unwrap_or(Self::DEFAULT_PER_PAGE)
                .clamp(1, Self::MAX_PER_PAGE),
        }
    }

    /// The SQL `LIMIT`: `per_page`.
    #[must_use]
    pub fn limit(self) -> i64 {
        i64::from(self.per_page)
    }

    /// The SQL `OFFSET`: the rows on the pages before this one.
    #[must_use]
    pub fn offset(self) -> i64 {
        i64::from(self.page.saturating_sub(1)) * i64::from(self.per_page)
    }
}

impl Default for Pagination {
    /// The first page of [`Pagination::DEFAULT_PER_PAGE`] rows.
    fn default() -> Self {
        Pagination::new(None, None)
    }
}

impl From<Pagination> for Page {
    fn from(pagination: Pagination) -> Self {
        Page::new(pagination.limit(), pagination.offset())
    }
}
"#;

pub(super) const RESPONSE: &str = r#"//! What handlers answer besides a bare row: the wire standard's error body,
//! a page of a list, a path segment, and a versioned row's `ETag`.

use serde::{Deserialize, Serialize};

use super::pagination::Pagination;
pub use crate::db::http::{ErrorBody, ErrorPayload};

/// One page of a list: `{"items", "page", "per_page", "total"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaginatedResponse<T> {
    /// The page's rows.
    pub items: Vec<T>,
    /// The page, from 1.
    pub page: u32,
    /// The most rows a page holds.
    pub per_page: u32,
    /// The rows on every page together.
    pub total: i64,
}

impl<T> PaginatedResponse<T> {
    /// The page `pagination` asked for, holding `items` of `total` rows.
    #[must_use]
    pub fn new(items: Vec<T>, pagination: Pagination, total: i64) -> Self {
        PaginatedResponse {
            items,
            page: pagination.page,
            per_page: pagination.per_page,
            total,
        }
    }

    /// The same page with each item turned into another, such as a row
    /// into its response.
    #[must_use]
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> PaginatedResponse<U> {
        PaginatedResponse {
            items: self.items.into_iter().map(f).collect(),
            page: self.page,
            per_page: self.per_page,
            total: self.total,
        }
    }
}

/// `value` as one path segment, such as a key in a `Location`: every byte
/// but the unreserved characters (`A-Z`, `a-z`, `0-9`, `-`, `.`, `_`, `~`)
/// percent-encoded, so a `/` or a space in it stays inside the segment.
#[must_use]
pub fn path_segment(value: &impl std::fmt::Display) -> String {
    let text = value.to_string();
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// The `ETag` of a row at `version`: the version in quotes, which
/// `If-Match` sends back to update the row.
#[must_use]
pub fn etag(version: &impl std::fmt::Display) -> String {
    format!("\"{version}\"")
}
"#;

const TENANT: &str = r#"//! [`Tenant`]: the tenant of a request, as the auth middleware found it.

use std::future::{Ready, ready};
use std::ops::Deref;

use actix_web::dev::Payload;
use actix_web::{FromRequest, HttpMessage, HttpRequest};

use crate::error::AppError;

/// The tenant of a request, which the auth middleware stored in the
/// request's extensions (`req.extensions_mut().insert(tenant)`). Handlers of
/// tenant-scoped tables take one. A request without one is a wiring bug, not
/// the caller's fault: a `500` with `internal_error`.
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

impl<T: Clone + 'static> FromRequest for Tenant<T> {
    type Error = AppError;
    type Future = Ready<Result<Self, AppError>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let tenant = req.extensions().get::<T>().cloned();
        ready(tenant.map(Tenant).ok_or_else(|| {
            AppError::Internal(
                format!(
                    "no `{}` tenant in the request extensions: is the middleware that sets it mounted?",
                    std::any::type_name::<T>()
                )
                .into(),
            )
        }))
    }
}
"#;
