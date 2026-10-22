// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `api_keys` handlers: each parses the request, calls one
//! [`ApiKeyService`] method, and answers.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use super::dto::{ApiKeyResponse, CreateApiKeyDto, CreatedApiKeyResponse};
use super::service::ApiKeyService;
use crate::db::api_keys::models::ApiKeyId;
use crate::error::AppError;
use crate::shared::auth::AuthUser;
use crate::shared::response::ErrorBody;
use crate::shared::validation::{Path, ValidatedJson};

/// `GET /api/v1/api-keys`: the caller's active keys, newest first, `200`.
#[utoipa::path(
    get,
    path = "/api/v1/api-keys",
    tag = "api-keys",
    operation_id = "list",
    responses(
        (status = 200, description = "The caller's own active keys", body = [ApiKeyResponse]),
        (status = 401, description = "No valid bearer token", body = ErrorBody),
        (status = 403, description = "An API key can't manage keys", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn list(
    State(service): State<Arc<ApiKeyService>>,
    caller: AuthUser,
) -> Result<Response, AppError> {
    let keys = service.list(caller.id.into()).await?;
    Ok(Json(keys).into_response())
}

/// `POST /api/v1/api-keys`: a new key, answered once with its secret, `201`.
#[utoipa::path(
    post,
    path = "/api/v1/api-keys",
    tag = "api-keys",
    operation_id = "create",
    request_body = CreateApiKeyDto,
    responses(
        (status = 201, description = "Key created; the raw key is shown only in this response", body = CreatedApiKeyResponse),
        (status = 400, description = "A malformed body", body = ErrorBody),
        (status = 401, description = "No valid bearer token", body = ErrorBody),
        (status = 403, description = "An API key can't manage keys", body = ErrorBody),
        (status = 409, description = "The caller already has 25 active keys", body = ErrorBody),
        (status = 422, description = "A name or expiry out of bounds", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn create(
    State(service): State<Arc<ApiKeyService>>,
    caller: AuthUser,
    body: ValidatedJson<CreateApiKeyDto>,
) -> Result<Response, AppError> {
    let created = service.create(caller.id.into(), body.into_inner()).await?;
    Ok((StatusCode::CREATED, Json(created)).into_response())
}

/// `DELETE /api/v1/api-keys/{id}`: revokes one of the caller's keys, `204`.
#[utoipa::path(
    delete,
    path = "/api/v1/api-keys/{id}",
    tag = "api-keys",
    operation_id = "revoke",
    params(("id" = uuid::Uuid, Path, description = "API key id")),
    responses(
        (status = 204, description = "Key revoked"),
        (status = 401, description = "No valid bearer token", body = ErrorBody),
        (status = 403, description = "An API key can't manage keys", body = ErrorBody),
        (status = 404, description = "Key not found", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn revoke(
    State(service): State<Arc<ApiKeyService>>,
    caller: AuthUser,
    Path(id): Path<ApiKeyId>,
) -> Result<Response, AppError> {
    service.revoke(caller.id.into(), id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
