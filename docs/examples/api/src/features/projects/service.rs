// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! `ProjectsService`: the `projects` feature's business rules. Override any
//! [`ProjectsServiceApi`] method here; the others keep their generated defaults.
//! A port this feature needs from another feature is a trait declared here
//! and implemented in `app.rs`.

use sqlx::{PgConnection, PgPool};

use super::model::ProjectId;
use super::service_api::ProjectsServiceApi;
use crate::db::queries;
use crate::error::AppError;
use crate::shared::auth::AuthUser;

/// The `projects` service, registered as app data by `register_services`.
pub struct ProjectsService {
    /// The pool the service reads and writes through.
    pub pool: PgPool,
}

impl ProjectsService {
    /// The service over `pool`, as `register_services` builds it. Give it
    /// what else it needs (a port) here.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        ProjectsService { pool }
    }

    /// Counts one more open task on the caller's project `id`, on `conn`.
    /// The tasks feature calls this through its `ProjectTasks` port, inside
    /// the transaction that creates the task. Another caller's project is a
    /// `404`.
    pub async fn open_task_in(
        &self,
        conn: &mut PgConnection,
        user: AuthUser,
        id: ProjectId,
    ) -> Result<(), AppError> {
        let updated = queries::open_task(&mut *conn, id, user.id).await?;
        if updated == 0 {
            return Err(AppError::NotFound("no such project".to_owned()));
        }
        Ok(())
    }
}

impl ProjectsServiceApi for ProjectsService {
    fn pool(&self) -> &PgPool {
        &self.pool
    }
}
