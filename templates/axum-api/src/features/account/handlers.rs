// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `account` handlers: each parses the request, calls one
//! [`AccountService`] method, and answers.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};

use super::dto::{UpdateProfileDto, UserResponse};
use super::service::AccountService;
use crate::error::AppError;
use crate::shared::auth::AuthUser;
use crate::shared::response::ErrorBody;
use crate::shared::validation::ValidatedJson;

/// `GET /api/v1/auth/me`: the signed-in user, `200`.
#[utoipa::path(
    get,
    path = "/api/v1/auth/me",
    tag = "auth",
    operation_id = "me",
    responses(
        (status = 200, description = "The signed-in user", body = UserResponse),
        (status = 401, description = "No valid bearer token", body = ErrorBody),
        (status = 429, description = "Too many requests from this IP", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn auth_me(
    State(service): State<Arc<AccountService>>,
    caller: AuthUser,
) -> Result<Response, AppError> {
    let user = service.me(caller.id.into()).await?;
    Ok(Json(UserResponse::from(user)).into_response())
}

/// `GET /api/v1/users/me`: the signed-in user, `200`.
#[utoipa::path(
    get,
    path = "/api/v1/users/me",
    tag = "users",
    operation_id = "me",
    responses(
        (status = 200, description = "The signed-in user", body = UserResponse),
        (status = 401, description = "No valid bearer token", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn get_me(
    State(service): State<Arc<AccountService>>,
    caller: AuthUser,
) -> Result<Response, AppError> {
    let user = service.me(caller.id.into()).await?;
    Ok(Json(UserResponse::from(user)).into_response())
}

/// `PATCH /api/v1/users/me`: changes the signed-in user's profile, `200`.
#[utoipa::path(
    patch,
    path = "/api/v1/users/me",
    tag = "users",
    operation_id = "update_me",
    request_body = UpdateProfileDto,
    responses(
        (status = 200, description = "Updated profile", body = UserResponse),
        (status = 400, description = "A malformed body", body = ErrorBody),
        (status = 401, description = "No valid bearer token", body = ErrorBody),
        (status = 422, description = "A display name over 100 characters", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn update_me(
    State(service): State<Arc<AccountService>>,
    caller: AuthUser,
    body: ValidatedJson<UpdateProfileDto>,
) -> Result<Response, AppError> {
    let user = service
        .update_profile(caller.id.into(), body.into_inner())
        .await?;
    Ok(Json(UserResponse::from(user)).into_response())
}
