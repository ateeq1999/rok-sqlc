//! The README's validated-bodies example: `derive = ["serde", "validate"]`
//! and `extractors = ["axum", "actix_web"]` (see `rok-sqlc.toml`) over
//! `migrations/`.

#![deny(missing_docs)]

pub mod db;

use db::members::models::{Member, NewMember};
use sqlx::PgPool;

/// `POST /members` on axum.
pub mod with_axum {
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::routing::post;
    use axum::{Json, Router};

    use super::{Member, NewMember, PgPool};
    use crate::db::http::axum::ValidatedJson;

    /// The routes, sharing `pool`.
    pub fn router(pool: PgPool) -> Router {
        Router::new()
            .route("/members", post(create_member))
            .with_state(pool)
    }

    /// Runs only for a validated body. Anything else was already answered
    /// `400`, `413` or `422` in the Rok error envelope.
    async fn create_member(
        State(pool): State<PgPool>,
        ValidatedJson(input): ValidatedJson<NewMember>,
    ) -> Result<Json<Member>, StatusCode> {
        let member = crate::db::members::write::create(&pool, input)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Json(member))
    }
}

/// `POST /members` on actix-web.
pub mod with_actix_web {
    use actix_web::error::ErrorInternalServerError;
    use actix_web::web;

    use super::{Member, NewMember, PgPool};
    use crate::db::http::actix_web::ValidatedJson;

    /// Adds the routes to an `App` that holds a `web::Data<PgPool>`.
    pub fn configure(config: &mut web::ServiceConfig) {
        config.route("/members", web::post().to(create_member));
    }

    /// Runs only for a validated body. No `JsonConfig` error handler is
    /// needed: the extractor's rejection answers in the Rok error envelope.
    async fn create_member(
        pool: web::Data<PgPool>,
        ValidatedJson(input): ValidatedJson<NewMember>,
    ) -> actix_web::Result<web::Json<Member>> {
        let member = crate::db::members::write::create(pool.get_ref(), input)
            .await
            .map_err(ErrorInternalServerError)?;
        Ok(web::Json(member))
    }
}
