// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Builds the application: its state, the generated features' services and
//! routes, health, and the `OpenAPI` document. This is the only wiring file;
//! the ports features declare for each other are implemented here too.

use actix_web::{App, HttpServer, web};
use sqlx::{PgConnection, PgPool};
use utoipa_swagger_ui::SwaggerUi;

use crate::config::Config;
use crate::db::pool::init_pool;
use crate::db::projects::models::ProjectId;
use crate::error::AppError;
use crate::features::projects::ProjectsService;
use crate::features::tasks::service::ProjectTasks;
use crate::features::{configure_all, register_services};
use crate::openapi::document;
use crate::shared::auth::{AuthUser, JwtKeys};
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

/// The tasks feature's [`ProjectTasks`] port, answered by the projects
/// feature's service on the caller's connection.
pub struct ProjectTasksPort {
    projects: ProjectsService,
}

impl ProjectTasksPort {
    /// The port over `pool`.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        ProjectTasksPort {
            projects: ProjectsService::new(pool),
        }
    }
}

#[async_trait::async_trait]
impl ProjectTasks for ProjectTasksPort {
    async fn task_opened(
        &self,
        conn: &mut PgConnection,
        user: AuthUser,
        project: ProjectId,
    ) -> Result<(), AppError> {
        self.projects.open_task_in(conn, user, project).await
    }
}
