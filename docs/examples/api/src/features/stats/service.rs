//! `StatsService`: reads the caller's statistics through the data layer.

use sqlx::PgPool;

use crate::db::queries::{self, ProjectStatsRow};
use crate::error::AppError;
use crate::shared::auth::AuthUser;

/// The `stats` service, registered as app data in `app.rs`.
pub struct StatsService {
    /// The pool the service reads through.
    pool: PgPool,
}

impl StatsService {
    /// The service over `pool`.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        StatsService { pool }
    }

    /// The caller's projects and open tasks.
    pub async fn summary(&self, user: AuthUser) -> Result<ProjectStatsRow, AppError> {
        let row = queries::project_stats(&self.pool, user.id).await?;
        Ok(row)
    }
}
