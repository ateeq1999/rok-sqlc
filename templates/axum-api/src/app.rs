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
use axum::extract::{DefaultBodyLimit, FromRef};
use axum::http::StatusCode;
use sqlx::{PgConnection, PgPool};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
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
use crate::features::{Services, register_services, routes};
use crate::openapi::document;
use crate::shared::health;
use crate::shared::mail::{Mailer, Outbox, Sender, Templates, transport};
use crate::shared::rate_limit::RateLimit;
use crate::shared::security::breach::{self, BreachCheck};
use crate::shared::security::jwt::JwtKeys;

/// The largest request body: 2 MiB. A JSON body over it is a `413`.
const BODY_LIMIT: usize = 2 * 1024 * 1024;

/// How long a request may take before it is answered `408`.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// What the handlers reach through `State`, built once: by [`run`], or by a
/// test. Each field is an extractable state of its own (`FromRef`); the
/// generated `features/generated.rs` hands each handler its service from
/// `services`.
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
    /// Checks new passwords against known breaches (`BREACH_CHECK`); a
    /// test swaps in its own.
    pub breach: Arc<dyn BreachCheck>,
    /// The per-IP limit for `/auth` (`AUTH_RATE_LIMIT`, `TRUST_PROXY`),
    /// one for every route it wraps.
    pub rate_limit: RateLimit,
    /// `REQUIRE_VERIFIED_EMAIL`.
    #[from_ref(skip)]
    pub require_verified_email: bool,
    /// Where `auth` records what admins do: the `audit_log` feature, or a
    /// test's own.
    pub audit: Arc<dyn AuditTrail>,
}

impl AppState {
    /// The state `config` describes, over `pool`.
    #[must_use]
    pub fn new(pool: PgPool, config: &Config) -> Self {
        AppState {
            services: register_services(&pool),
            audit: Arc::new(AuditLog(AuditLogEntryService::new(pool.clone()))),
            pool,
            keys: Arc::new(JwtKeys::new(&config.jwt_secret, config.jwt_ttl)),
            outbox: Outbox::new(),
            templates: Arc::new(Templates::new(&config.app_url)),
            breach: breach::from_config(config),
            rate_limit: RateLimit::per_minute(config.auth_rate_limit, config.trust_proxy),
            require_verified_email: config.require_verified_email,
        }
    }
}

/// Connects the pool, then serves the application on `config.bind_addr`
/// (see [`serve`]) until Ctrl+C or `SIGTERM`.
pub async fn run(config: Config) -> std::io::Result<()> {
    let pool = init_pool(&config.database_url, &config.pool)
        .await
        .map_err(std::io::Error::other)?;
    let mailer = transport::from_config(&config).map_err(std::io::Error::other)?;
    let state = AppState::new(pool, &config);
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
        .merge(routes())
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            REQUEST_TIMEOUT,
        ))
        // Same-origin only: allow your front end's origin here.
        .layer(CorsLayer::new())
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(TraceLayer::new_for_http())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .with_state(state)
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

/// `auth`'s service, built from `state` for each request that takes it, so
/// a test's changes to `state` (its `breach` or `audit`) apply.
impl FromRef<AppState> for Arc<AuthService> {
    fn from_ref(state: &AppState) -> Self {
        Arc::new(AuthService {
            pool: state.pool.clone(),
            keys: state.keys.clone(),
            outbox: Arc::new(state.outbox.clone()),
            templates: state.templates.clone(),
            breach: state.breach.clone(),
            require_verified_email: state.require_verified_email,
            audit: state.audit.clone(),
        })
    }
}

/// `account`'s service, over `state`'s pool.
impl FromRef<AppState> for Arc<AccountService> {
    fn from_ref(state: &AppState) -> Self {
        Arc::new(AccountService::new(state.pool.clone()))
    }
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
