//! The `stats` handler.

use actix_web::{HttpResponse, web};

use super::dto::StatsResponse;
use super::service::StatsService;
use crate::error::AppError;
use crate::shared::auth::AuthUser;
use crate::shared::response::ErrorBody;

/// `GET /api/v1/stats`: the caller's statistics, `200`.
#[utoipa::path(
    get,
    path = "/api/v1/stats",
    tag = "stats",
    operation_id = "summary",
    responses(
        (status = 200, description = "The caller's statistics.", body = StatsResponse),
        (status = 401, description = "No valid bearer token.", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn summary(
    service: web::Data<StatsService>,
    user: AuthUser,
) -> Result<HttpResponse, AppError> {
    let row = service.summary(user).await?;
    Ok(HttpResponse::Ok().json(StatsResponse::from(row)))
}
