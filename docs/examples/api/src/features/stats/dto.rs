//! The `stats` response body.

use crate::db::queries::ProjectStatsRow;

/// `GET /api/v1/stats`'s answer: the caller's projects and their open
/// tasks.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct StatsResponse {
    /// How many projects the caller has.
    pub projects: i64,
    /// The open tasks of all of them together.
    pub open_tasks: i64,
}

impl From<ProjectStatsRow> for StatsResponse {
    fn from(row: ProjectStatsRow) -> Self {
        StatsResponse {
            projects: row.projects,
            open_tasks: row.open_tasks.unwrap_or(0),
        }
    }
}
