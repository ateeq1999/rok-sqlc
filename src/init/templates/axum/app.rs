//! Builds the application: its state, the generated features' services and
//! routes, health, the `OpenAPI` document, and the middleware. This is the
//! only wiring file; the ports features declare for each other are
//! implemented here too.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{DefaultBodyLimit, FromRef, Request, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use sqlx::PgPool;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use utoipa_swagger_ui::SwaggerUi;

use crate::config::Config;
use crate::db::pool::init_pool;
use crate::error::AppError;
use crate::features::{Services, register_services, routes};
use crate::openapi::document;
use crate::shared::auth::JwtKeys;
use crate::shared::health;

/// The largest request body: 2 MiB. A JSON body over it is a `413`.
const BODY_LIMIT: usize = 2 * 1024 * 1024;

/// How long a request may take before it is answered `408`.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// What the handlers reach through `State`: the pool, every feature's
/// service, and the JWT keys. Each field is an extractable state of its
/// own (`FromRef`); the generated `features/generated.rs` hands each
/// handler its service from `services`.
#[derive(Clone, FromRef)]
pub struct AppState {
    /// The database pool, which `/health/ready` checks.
    pub pool: PgPool,
    /// Every generated feature's service.
    pub services: Services,
    /// The keys `AuthUser` and `AdminUser` verify tokens with.
    pub keys: Arc<JwtKeys>,
}

impl AppState {
    /// The state over `pool`, verifying tokens with `keys`.
    #[must_use]
    pub fn new(pool: PgPool, keys: JwtKeys) -> Self {
        let services = register_services(&pool);
        AppState {
            pool,
            services,
            keys: Arc::new(keys),
        }
    }
}

/// Connects the pool, then serves the application on `config.bind_addr`
/// until Ctrl+C or `SIGTERM`, letting requests in flight finish.
pub async fn run(config: Config) -> std::io::Result<()> {
    let pool = init_pool(&config.database_url, &config.pool)
        .await
        .map_err(std::io::Error::other)?;
    let state = AppState::new(pool, JwtKeys::new(&config.jwt_secret));
    let listener = TcpListener::bind(&config.bind_addr).await?;
    tracing::info!(addr = %config.bind_addr, "listening");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
}

/// The whole application over `state`: `/health/live` and
/// `/health/ready`, the `OpenAPI` document at `/api-docs/openapi.json` with
/// Swagger UI at `/docs/`, every feature's routes, and the middleware:
/// tracing, request ids (`x-request-id`), CORS, a timeout and a body
/// limit. Tests drive the same router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(health::routes())
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", document()))
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
