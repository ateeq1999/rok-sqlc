// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `account` handlers: each parses the request, calls one
//! [`AccountService`] method, and answers.

use actix_web::http::header;
use actix_web::{HttpResponse, web};

use super::dto::{AVATAR_CACHE_CONTROL, AVATAR_MAX_BYTES, UpdateProfileDto, UserResponse};
use super::service::AccountService;
use crate::error::AppError;
use crate::shared::auth::AuthUser;
use crate::shared::body;
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
    service: web::Data<AccountService>,
    caller: AuthUser,
) -> Result<HttpResponse, AppError> {
    let user = service.me(caller.id.into()).await?;
    Ok(HttpResponse::Ok().json(UserResponse::from(user)))
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
    service: web::Data<AccountService>,
    caller: AuthUser,
) -> Result<HttpResponse, AppError> {
    let user = service.me(caller.id.into()).await?;
    Ok(HttpResponse::Ok().json(UserResponse::from(user)))
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
    service: web::Data<AccountService>,
    caller: AuthUser,
    body: ValidatedJson<UpdateProfileDto>,
) -> Result<HttpResponse, AppError> {
    let user = service
        .update_profile(caller.id.into(), body.into_inner())
        .await?;
    Ok(HttpResponse::Ok().json(UserResponse::from(user)))
}

/// `PUT /api/v1/users/me/avatar`: makes the raw body (a JPEG, PNG or WebP
/// image) the signed-in user's profile photo, `200`.
#[utoipa::path(
    put,
    path = "/api/v1/users/me/avatar",
    tag = "avatars",
    operation_id = "upload",
    request_body(
        content_type = "application/octet-stream",
        description = "The raw image bytes (JPEG, PNG or WebP), 2 MiB at most"
    ),
    responses(
        (status = 200, description = "Updated profile, with the new avatar_url", body = UserResponse),
        (status = 401, description = "No valid bearer token", body = ErrorBody),
        (status = 413, description = "Larger than 2 MiB", body = ErrorBody),
        (status = 422, description = "Not a decodable JPEG, PNG or WebP image, or too large to process", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn upload_avatar(
    service: web::Data<AccountService>,
    caller: AuthUser,
    body: Result<web::Bytes, actix_web::Error>,
) -> Result<HttpResponse, AppError> {
    let image = body::limited(body, AVATAR_MAX_BYTES, "image")?;
    let user = service.set_avatar(caller.id.into(), image).await?;
    Ok(HttpResponse::Ok().json(UserResponse::from(user)))
}

/// `DELETE /api/v1/users/me/avatar`: removes the signed-in user's profile
/// photo, `200`.
#[utoipa::path(
    delete,
    path = "/api/v1/users/me/avatar",
    tag = "avatars",
    operation_id = "remove",
    responses(
        (status = 200, description = "Updated profile, avatar_url now null", body = UserResponse),
        (status = 401, description = "No valid bearer token", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn remove_avatar(
    service: web::Data<AccountService>,
    caller: AuthUser,
) -> Result<HttpResponse, AppError> {
    let user = service.remove_avatar(caller.id.into()).await?;
    Ok(HttpResponse::Ok().json(UserResponse::from(user)))
}

/// `GET /api/v1/avatars/{file}`: a profile photo, public and cacheable for
/// good, `200`.
#[utoipa::path(
    get,
    path = "/api/v1/avatars/{file}",
    tag = "avatars",
    operation_id = "serve",
    params(("file" = String, Path, description = "Generated avatar file name")),
    responses(
        (status = 200, description = "The JPEG image", content_type = "image/jpeg"),
        (status = 404, description = "No such file", body = ErrorBody),
    ),
)]
pub async fn avatar(
    service: web::Data<AccountService>,
    file: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let jpeg = service.avatar(&file.into_inner()).await?;
    Ok(HttpResponse::Ok()
        .insert_header((header::CONTENT_TYPE, "image/jpeg"))
        .insert_header((header::CACHE_CONTROL, AVATAR_CACHE_CONTROL))
        .insert_header((header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .body(jpeg))
}
