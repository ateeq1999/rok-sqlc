// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Builds the application: its state, the generated features' services and
//! routes, health, and the `OpenAPI` document. This is the only wiring file;
//! the ports features declare for each other are implemented here too.

use actix_web::{App, HttpServer, web};
use sqlx::PgPool;
use utoipa_swagger_ui::SwaggerUi;

use crate::config::Config;
use crate::db::pool::init_pool;
use crate::features::{configure_all, register_services};
use crate::openapi::document;
use crate::shared::health;
use crate::shared::mail::{Outbox, Sender, Templates, transport};
use crate::shared::security::jwt::JwtKeys;

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
}

impl AppState {
    /// The state `config` describes, over `pool`.
    #[must_use]
    pub fn new(pool: PgPool, config: &Config) -> Self {
        AppState {
            pool,
            keys: web::Data::new(JwtKeys::new(&config.jwt_secret, config.jwt_ttl)),
            outbox: web::Data::new(Outbox::new()),
            templates: web::Data::new(Templates::new(&config.app_url)),
        }
    }
}

/// Connects the pool, starts the mail sender, then serves the application
/// on `config.bind_addr` until the server stops, and stops the sender.
pub async fn run(config: Config) -> std::io::Result<()> {
    let pool = init_pool(&config.database_url, &config.pool)
        .await
        .map_err(std::io::Error::other)?;
    let mailer = transport::from_config(&config).map_err(std::io::Error::other)?;
    let state = AppState::new(pool, &config);
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
/// routes. Tests build the same application with
/// `actix_web::test::init_service(App::new().configure(...))`.
pub fn configure(cfg: &mut web::ServiceConfig, state: &AppState) {
    cfg.app_data(web::Data::new(state.pool.clone()))
        .app_data(state.keys.clone())
        .app_data(state.outbox.clone())
        .app_data(state.templates.clone());
    register_services(cfg, &state.pool);
    health::routes(cfg);
    cfg.service(web::redirect("/docs", "/docs/").see_other())
        .service(SwaggerUi::new("/docs/{_:.*}").url("/api-docs/openapi.json", document()));
    configure_all(cfg);
}
