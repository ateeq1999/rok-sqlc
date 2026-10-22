// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Builds the application: its state, the generated features' services and
//! routes, health, the `OpenAPI` document, and the middleware; and runs
//! it, with the mail sender beside it. This is the only wiring file; the
//! ports features declare for each other are implemented here too.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::Router;
use axum::extract::{DefaultBodyLimit, FromRef, Request, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use sqlx::{PgConnection, PgPool};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use utoipa_swagger_ui::SwaggerUi;

use crate::config::Config;
use crate::db::audit_log_entry::models::NewAuditLogEntry;
use crate::db::pool::init_pool;
use crate::error::AppError;
use crate::features::account::{self, AccountService};
use crate::features::audit_log_entry::AuditLogEntryService;
use crate::features::auth::service::{AuditEntry, AuditTrail};
use crate::features::auth::{self, AuthService};
use crate::features::media::{self, MediaService};
use crate::features::{Services, register_services, routes};
use crate::openapi::document;
use crate::shared::health;
use crate::shared::mail::{Mailer, Outbox, Sender, Templates, transport};
use crate::shared::rate_limit::RateLimit;
use crate::shared::security::breach::{self, BreachCheck};
use crate::shared::security::jwt::JwtKeys;
use crate::shared::storage::{self, ObjectStorage, Storage};

/// The largest request body: 2 MiB. A JSON body over it is a `413`.
const BODY_LIMIT: usize = 2 * 1024 * 1024;

/// How long a request may take before it is answered `408`.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// What the handlers reach through `State`, built once: by [`run`], or by a
/// test. Each field is an extractable state of its own (`FromRef`); the
/// generated `features/generated.rs` hands each handler its service from
/// `services`, and the hand-written `auth` and `account` handlers take
/// theirs from `auth` and `account`. Every service is built here, once,
/// not per request.
#[derive(Clone, FromRef)]
pub struct AppState {
    /// The database, which `/health/ready` checks and `AuthUser` reads the
    /// session from.
    pub pool: PgPool,
    /// Every generated feature's service.
    pub services: Services,
    /// Signs and verifies session tokens.
    pub keys: Arc<JwtKeys>,
    /// Where flows enqueue their mail; it wakes the sender.
    pub outbox: Outbox,
    /// The messages flows enqueue, with links under `APP_URL`.
    pub templates: Arc<Templates>,
    /// The per-IP limit for `/auth` (`AUTH_RATE_LIMIT`, `TRUST_PROXY`),
    /// one for every route it wraps.
    pub rate_limit: RateLimit,
    /// `auth`'s service, over the parts above, the breach check
    /// (`BREACH_CHECK`), `REQUIRE_VERIFIED_EMAIL` and the audit trail (the
    /// `audit_log` feature). A test swaps the breach check or the audit
    /// trail with [`AppState::with_breach`] or [`AppState::with_audit`].
    pub auth: Arc<AuthService>,
    /// `account`'s service.
    pub account: Arc<AccountService>,
    /// Where uploaded bytes are stored: in memory from [`AppState::new`];
    /// [`run`] opens `STORAGE_URL` with [`AppState::with_storage`].
    pub storage: Arc<dyn Storage>,
    /// `media`'s service, over `storage` and the `MEDIA_*` settings.
    pub media: Arc<MediaService>,
}

impl AppState {
    /// The state `config` describes, over `pool`.
    #[must_use]
    pub fn new(pool: PgPool, config: &Config) -> Self {
        let keys = Arc::new(JwtKeys::new(&config.jwt_secret, config.jwt_ttl));
        let outbox = Outbox::new();
        let templates = Arc::new(Templates::new(&config.app_url));
        let auth = Arc::new(AuthService {
            pool: pool.clone(),
            keys: keys.clone(),
            outbox: Arc::new(outbox.clone()),
            templates: templates.clone(),
            breach: breach::from_config(config),
            require_verified_email: config.require_verified_email,
            audit: Arc::new(AuditLog(AuditLogEntryService::new(pool.clone()))),
        });
        let storage: Arc<dyn Storage> = Arc::new(ObjectStorage::memory());
        let media = Arc::new(MediaService {
            pool: pool.clone(),
            storage: storage.clone(),
            max_upload_bytes: config.media_max_upload_bytes,
            allowed_types: config.media_allowed_types.clone(),
        });
        AppState {
            services: register_services(&pool),
            storage,
            media,
            account: Arc::new(AccountService::new(pool.clone())),
            auth,
            pool,
            keys,
            outbox,
            templates,
            rate_limit: RateLimit::per_minute(config.auth_rate_limit, config.trust_proxy),
        }
    }

    /// This state, its `auth` checking new passwords with `breach`.
    #[must_use]
    pub fn with_breach(mut self, breach: Arc<dyn BreachCheck>) -> Self {
        Arc::make_mut(&mut self.auth).breach = breach;
        self
    }

    /// This state, storing uploads in `storage`.
    #[must_use]
    pub fn with_storage(mut self, storage: Arc<dyn Storage>) -> Self {
        Arc::make_mut(&mut self.media).storage = storage.clone();
        self.storage = storage;
        self
    }

    /// This state, its `auth` recording what admins do with `audit`.
    #[must_use]
    pub fn with_audit(mut self, audit: Arc<dyn AuditTrail>) -> Self {
        Arc::make_mut(&mut self.auth).audit = audit;
        self
    }
}

/// Connects the pool, then serves the application on `config.bind_addr`
/// (see [`serve`]) until Ctrl+C or `SIGTERM`.
pub async fn run(config: Config) -> std::io::Result<()> {
    let pool = init_pool(&config.database_url, &config.pool)
        .await
        .map_err(std::io::Error::other)?;
    let mailer = transport::from_config(&config).map_err(std::io::Error::other)?;
    let storage = storage::open(&config.storage_url).map_err(std::io::Error::other)?;
    let state = AppState::new(pool, &config).with_storage(storage);
    let listener = TcpListener::bind(&config.bind_addr).await?;
    tracing::info!(addr = %config.bind_addr, "listening");
    serve(listener, state, mailer, shutdown_signal()).await
}

/// Serves [`router`] over `state` on `listener`, with the mail sender
/// delivering `state.outbox` through `mailer` beside it (`tokio::spawn`),
/// until `shutdown` completes: then requests in flight finish, the server
/// returns, and the sender stops after its pass in progress.
pub async fn serve(
    listener: TcpListener,
    state: AppState,
    mailer: Arc<dyn Mailer>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let sender = Sender::new(state.pool.clone(), mailer, &state.outbox);
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let sending = tokio::spawn(sender.run(async {
        // A dropped `stop` stops the sender too.
        let _ = stopped.await;
    }));
    let app = router(state).into_make_service_with_connect_info::<SocketAddr>();
    let served = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await;
    let _ = stop.send(());
    if let Err(err) = sending.await {
        tracing::error!(error = %err, "the mail sender failed");
    }
    served
}

/// The whole application over `state`: `/health/live` and
/// `/health/ready`, the `OpenAPI` document at `/api-docs/openapi.json` with
/// Swagger UI at `/docs/`, every feature's routes (the hand-written `account` and
/// `auth`, their `/auth` routes behind `state.rate_limit`), and the middleware:
/// tracing, request ids (`x-request-id`), CORS, a timeout and a body
/// limit. Tests drive the same router.
///
/// Mount a hand-written feature by merging its `routes()` here; wrap the
/// routes that need it in `state.rate_limit.clone()` (a `Layer`).
pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(health::routes())
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", document()))
        .merge(account::routes(&state.rate_limit))
        .merge(auth::routes(&state.rate_limit))
        .merge(media::routes(state.media.max_upload_bytes))
        .merge(routes())
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(middleware::from_fn_with_state(REQUEST_TIMEOUT, time_out))
        // Same-origin only: allow your front end's origin here.
        .layer(CorsLayer::new())
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(TraceLayer::new_for_http())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .with_state(state)
}

/// The request timeout [`router`] applies (`from_fn_with_state(limit,
/// time_out)`): a request still running after `limit` is dropped and
/// answered `408 request_timeout` with the wire standard's body.
pub async fn time_out(State(limit): State<Duration>, request: Request, next: Next) -> Response {
    match tokio::time::timeout(limit, next.run(request)).await {
        Ok(response) => response,
        Err(_) => AppError::RequestTimeout(format!("the request took longer than {limit:?}"))
            .into_response(),
    }
}

/// Resolves on Ctrl+C or, on Unix, `SIGTERM`.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "could not listen for Ctrl+C");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                terminate.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "could not listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
    tracing::info!("shutting down");
}

/// `auth`'s [`AuditTrail`] port onto the `audit_log` feature: each entry is
/// written by [`AuditLogEntryService::create_in`] on the flow's own
/// connection, so it commits with the change it records.
pub struct AuditLog(pub AuditLogEntryService);

#[async_trait]
impl AuditTrail for AuditLog {
    async fn record_in(&self, conn: &mut PgConnection, entry: AuditEntry) -> Result<(), AppError> {
        self.0
            .create_in(
                conn,
                NewAuditLogEntry {
                    id: None,
                    actor_user_id: entry.actor,
                    target_user_id: entry.target,
                    action: entry.action,
                    details: None,
                },
            )
            .await?;
        Ok(())
    }
}
