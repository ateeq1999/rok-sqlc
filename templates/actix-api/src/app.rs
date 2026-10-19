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
use crate::shared::auth::JwtKeys;
use crate::shared::health;

/// Connects the pool, then serves the application on `config.bind_addr`
/// until the server stops.
pub async fn run(config: Config) -> std::io::Result<()> {
    let pool = init_pool(&config.database_url, &config.pool)
        .await
        .map_err(std::io::Error::other)?;
    let keys = web::Data::new(JwtKeys::new(&config.jwt_secret));
    tracing::info!(addr = %config.bind_addr, "listening");
    HttpServer::new(move || App::new().configure(|cfg| configure(cfg, &pool, &keys)))
        .bind(&config.bind_addr)?
        .run()
        .await
}

/// Mounts the whole application on `cfg`: the pool and the JWT keys as app
/// data, every feature's service, `/health/live` and `/health/ready`, the
/// `OpenAPI` document at `/api-docs/openapi.json` with Swagger UI at `/docs/`,
/// and every feature's routes. Tests build the same application with
/// `actix_web::test::init_service(App::new().configure(...))`.
pub fn configure(cfg: &mut web::ServiceConfig, pool: &PgPool, keys: &web::Data<JwtKeys>) {
    cfg.app_data(web::Data::new(pool.clone()))
        .app_data(keys.clone());
    register_services(cfg, pool);
    health::routes(cfg);
    cfg.service(web::redirect("/docs", "/docs/").see_other())
        .service(SwaggerUi::new("/docs/{_:.*}").url("/api-docs/openapi.json", document()));
    configure_all(cfg);
}
