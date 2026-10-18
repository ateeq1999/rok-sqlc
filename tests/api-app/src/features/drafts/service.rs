// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! `DraftsService`: the `drafts` feature's business rules. Override any
//! [`DraftsServiceApi`] method here; the others keep their generated
//! defaults. A port this feature needs from another feature is a trait
//! declared here and implemented in `app.rs`.
//!
//! The fixture's own edit: `create_in` trims the title and refuses a blank
//! one. It was written while `drafts` had no key, and still compiles once
//! `0002_draft_keys.sql` adds one and the trait gains `get_one`, `update`
//! and `remove`.

use sqlx::{PgConnection, PgPool};

use super::dto::CreateDraftDto;
use super::model::Draft;
use super::repo;
use super::service_api::DraftsServiceApi;
use crate::error::AppError;

/// The `drafts` service, registered as app data by `register_services`.
pub struct DraftsService {
    /// The pool the service reads and writes through.
    pub pool: PgPool,
}

impl DraftsService {
    /// The service over `pool`, as `register_services` builds it. Give it
    /// what else it needs (a port) here.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        DraftsService { pool }
    }
}

impl DraftsServiceApi for DraftsService {
    fn pool(&self) -> &PgPool {
        &self.pool
    }

    async fn create_in(
        &self,
        conn: &mut PgConnection,
        input: CreateDraftDto,
    ) -> Result<Draft, AppError> {
        let title = input.title.trim().to_owned();
        if title.is_empty() {
            return Err(AppError::Validation {
                message: "a draft needs a title".to_owned(),
                details: None,
            });
        }
        Ok(repo::create(conn, CreateDraftDto { title }.into()).await?)
    }
}
