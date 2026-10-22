// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Builds the application: its state, the generated features' services and
//! routes, health, and the `OpenAPI` document. This is the only wiring file;
//! the ports features declare for each other are implemented here too.

use actix_web::{App, HttpServer, web};
use async_trait::async_trait;
use sqlx::{PgConnection, PgPool};
use utoipa_swagger_ui::SwaggerUi;

use std::sync::Arc;

use crate::config::Config;
use crate::db::audit_log_entry::models::NewAuditLogEntry;
use crate::db::pool::init_pool;
use crate::error::AppError;
use crate::features::account::{self, AccountService};
use crate::features::api_keys::{self, ApiKeyService};
use crate::features::audit_log_entry::AuditLogEntryService;
use crate::features::auth::service::{AuditEntry, AuditTrail};
use crate::features::auth::{self, AuthService};
use crate::features::media::{self, MediaService};
use crate::features::{configure_all, register_services};
use crate::openapi::document;
use crate::shared::health;
use crate::shared::mail::{Outbox, Sender, Templates, transport};
use crate::shared::rate_limit::RateLimit;
use crate::shared::security::breach::{self, BreachCheck};
use crate::shared::security::jwt::JwtKeys;
use crate::shared::storage::{self, ObjectStorage, Storage};

/// What every worker shares, built once: by [`run`], or by a test.
#[derive(Clone)]
pub struct AppState {
    /// The database.
    pub pool: PgPool,
    /// Signs and verifies session tokens.
    pub keys: web::Data<JwtKeys>,
    /// Where flows enqueue their mail; it wakes the sender.
    pub outbox: web::Data<Outbox>,
    /// The messages flows enqueue, with links under `APP_URL`.
    pub templates: web::Data<Templates>,
    /// Checks new passwords against known breaches (`BREACH_CHECK`); a
    /// test swaps in its own.
    pub breach: Arc<dyn BreachCheck>,
    /// The per-IP limit on `/auth` (`AUTH_RATE_LIMIT`, `TRUST_PROXY`),
    /// one for every worker.
    pub rate_limit: RateLimit,
    /// `REQUIRE_VERIFIED_EMAIL`.
    pub require_verified_email: bool,
    /// Where `auth` records what admins do: the `audit_log` feature, or a
    /// test's own.
    pub audit: Arc<dyn AuditTrail>,
    /// Where uploaded bytes are stored: in memory from [`AppState::new`];
    /// [`run`] opens `STORAGE_URL` with [`AppState::with_storage`].
    pub storage: Arc<dyn Storage>,
    /// `MEDIA_MAX_UPLOAD_BYTES`.
    pub media_max_upload_bytes: usize,
    /// `MEDIA_ALLOWED_TYPES`.
    pub media_allowed_types: Vec<String>,
}

impl AppState {
    /// The state `config` describes, over `pool`.
    #[must_use]
    pub fn new(pool: PgPool, config: &Config) -> Self {
        AppState {
            pool: pool.clone(),
            keys: web::Data::new(JwtKeys::new(&config.jwt_secret, config.jwt_ttl)),
            outbox: web::Data::new(Outbox::new()),
            templates: web::Data::new(Templates::new(&config.app_url)),
            breach: breach::from_config(config),
            rate_limit: RateLimit::per_minute(config.auth_rate_limit, config.trust_proxy),
            require_verified_email: config.require_verified_email,
            audit: Arc::new(AuditLog(AuditLogEntryService::new(pool))),
            storage: Arc::new(ObjectStorage::memory()),
            media_max_upload_bytes: config.media_max_upload_bytes,
            media_allowed_types: config.media_allowed_types.clone(),
        }
    }

    /// This state, storing uploads in `storage`.
    #[must_use]
    pub fn with_storage(mut self, storage: Arc<dyn Storage>) -> Self {
        self.storage = storage;
        self
    }
}

/// Connects the pool, starts the mail sender, then serves the application
/// on `config.bind_addr` until the server stops, and stops the sender.
pub async fn run(config: Config) -> std::io::Result<()> {
    let pool = init_pool(&config.database_url, &config.pool)
        .await
        .map_err(std::io::Error::other)?;
    let mailer = transport::from_config(&config).map_err(std::io::Error::other)?;
    let storage = storage::open(&config.storage_url).map_err(std::io::Error::other)?;
    let state = AppState::new(pool, &config).with_storage(storage);
    let sender = Sender::new(state.pool.clone(), mailer, &state.outbox);
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let sending = actix_web::rt::spawn(sender.run(async {
        // A dropped `stop` stops the sender too.
        let _ = stopped.await;
    }));
    tracing::info!(addr = %config.bind_addr, "listening");
    let server_state = state.clone();
    let served = HttpServer::new(move || App::new().configure(|cfg| configure(cfg, &server_state)))
        .bind(&config.bind_addr)?
        .run()
        .await;
    let _ = stop.send(());
    if let Err(err) = sending.await {
        tracing::error!(error = %err, "the mail sender failed");
    }
    served
}

/// Mounts the whole application on `cfg`: the pool, the JWT keys, the mail
/// outbox and templates as app data, every feature's service,
/// `/health/live` and `/health/ready`, the `OpenAPI` document at
/// `/api-docs/openapi.json` with Swagger UI at `/docs/`, and every feature's
/// routes: the hand-written `account` and `auth` first, so that
/// `/users/me` is not taken for `/users/{id}`. Tests build the same application with
/// `actix_web::test::init_service(App::new().configure(...))`.
pub fn configure(cfg: &mut web::ServiceConfig, state: &AppState) {
    cfg.app_data(web::Data::new(state.pool.clone()))
        .app_data(state.keys.clone())
        .app_data(state.outbox.clone())
        .app_data(state.templates.clone());
    register_services(cfg, &state.pool);
    cfg.app_data(web::Data::new(AccountService::new(
        state.pool.clone(),
        state.storage.clone(),
    )))
    .app_data(web::Data::new(AuthService {
        pool: state.pool.clone(),
        keys: state.keys.clone().into_inner(),
        outbox: state.outbox.clone().into_inner(),
        templates: state.templates.clone().into_inner(),
        breach: state.breach.clone(),
        require_verified_email: state.require_verified_email,
        audit: state.audit.clone(),
    }));
    cfg.app_data(web::Data::new(MediaService {
        pool: state.pool.clone(),
        storage: state.storage.clone(),
        max_upload_bytes: state.media_max_upload_bytes,
        allowed_types: state.media_allowed_types.clone(),
    }));
    health::routes(cfg);
    cfg.service(web::redirect("/docs", "/docs/").see_other())
        .service(SwaggerUi::new("/docs/{_:.*}").url("/api-docs/openapi.json", document()));
    cfg.app_data(web::Data::new(ApiKeyService::new(state.pool.clone())));
    account::routes(cfg, &state.rate_limit);
    api_keys::routes(cfg);
    auth::routes(cfg, &state.rate_limit);
    media::routes(cfg, state.media_max_upload_bytes);
    configure_all(cfg);
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
