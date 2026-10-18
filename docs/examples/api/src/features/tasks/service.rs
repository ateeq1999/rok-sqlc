// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! `TasksService`: the `tasks` feature's business rules. Override any
//! [`TasksServiceApi`] method here; the others keep their generated defaults.
//! A port this feature needs from another feature is a trait declared here
//! and implemented in `app.rs`.

use std::sync::Arc;

use sqlx::{PgConnection, PgPool};

use super::dto::CreateTaskDto;
use super::model::Task;
use super::repo;
use super::service_api::TasksServiceApi;
use crate::app::ProjectTasksPort;
use crate::db::projects::models::ProjectId;
use crate::error::AppError;
use crate::shared::auth::AuthUser;

/// What the tasks feature needs from the projects feature. `app.rs`
/// implements it.
#[async_trait::async_trait]
pub trait ProjectTasks: Send + Sync {
    /// Counts one more open task on the caller's `project`, on `conn`, or
    /// fails with `404` if the caller has no such project.
    async fn task_opened(
        &self,
        conn: &mut PgConnection,
        user: AuthUser,
        project: ProjectId,
    ) -> Result<(), AppError>;
}

/// The `tasks` service, registered as app data by `register_services`.
pub struct TasksService {
    /// The pool the service reads and writes through.
    pub pool: PgPool,
    /// The projects feature, through this feature's port.
    projects: Arc<dyn ProjectTasks>,
}

impl TasksService {
    /// The service over `pool`, as `register_services` builds it. Give it
    /// what else it needs (a port) here.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        let projects = Arc::new(ProjectTasksPort::new(pool.clone()));
        TasksService { pool, projects }
    }
}

impl TasksServiceApi for TasksService {
    fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// A task goes only into one of the caller's projects, and counts as
    /// one of its open tasks. Both writes share `conn`, so `create` commits
    /// them together.
    async fn create_in(
        &self,
        conn: &mut PgConnection,
        user: AuthUser,
        input: CreateTaskDto,
    ) -> Result<Task, AppError> {
        self.projects
            .task_opened(&mut *conn, user, input.project_id)
            .await?;
        let row = repo::create(&mut *conn, input.into_new(user.id)).await?;
        Ok(row)
    }
}
