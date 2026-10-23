// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `media` handlers: each parses the request, calls one
//! [`MediaService`] method, and answers.

use actix_web::http::header;
use actix_web::{HttpRequest, HttpResponse, web};

use super::dto::{ListMediaQuery, MediaResponse, UploadMediaQuery};
use super::service::MediaService;
use crate::error::AppError;
use crate::shared::auth::AuthUser;
use crate::shared::body;
use crate::shared::response::{ErrorBody, PaginatedResponse};

/// The header an upload may name its file in, when the query string
/// doesn't (`?filename=`).
const FILENAME_HEADER: &str = "x-filename";

/// The `Content-Security-Policy` of served files: nothing loads, and a
/// document opened anyway runs sandboxed.
const SANDBOX: &str = "default-src 'none'; sandbox";

/// `GET /api/v1/media`: one page of the caller's files, newest first, `200`.
#[utoipa::path(
    get,
    path = "/api/v1/media",
    tag = "media",
    operation_id = "list",
    params(ListMediaQuery),
    responses(
        (status = 200, description = "The caller's files, newest first", body = PaginatedResponse<MediaResponse>),
        (status = 400, description = "A malformed query", body = ErrorBody),
        (status = 401, description = "No valid bearer token", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn list(
    service: web::Data<MediaService>,
    caller: AuthUser,
    query: web::Query<ListMediaQuery>,
) -> Result<HttpResponse, AppError> {
    let page = service.list(caller.id.into(), query.pagination()).await?;
    Ok(HttpResponse::Ok().json(page))
}

/// `POST /api/v1/media`: stores the raw body as one of the caller's files,
/// `201`. The file name is `?filename=`, or else the `X-Filename` header.
#[utoipa::path(
    post,
    path = "/api/v1/media",
    tag = "media",
    operation_id = "upload",
    params(UploadMediaQuery),
    request_body(
        content_type = "application/octet-stream",
        description = "The raw file bytes. The type is detected from the bytes; the request's Content-Type is ignored"
    ),
    responses(
        (status = 201, description = "Stored", body = MediaResponse),
        (status = 400, description = "An empty body", body = ErrorBody),
        (status = 401, description = "No valid bearer token", body = ErrorBody),
        (status = 413, description = "Larger than MEDIA_MAX_UPLOAD_BYTES", body = ErrorBody),
        (status = 415, description = "Unrecognized file type, or one that is not allowed", body = ErrorBody),
        (status = 422, description = "A file name over 255 characters", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn upload(
    service: web::Data<MediaService>,
    caller: AuthUser,
    query: web::Query<UploadMediaQuery>,
    request: HttpRequest,
    body: Result<web::Bytes, actix_web::Error>,
) -> Result<HttpResponse, AppError> {
    let bytes = body::limited(body, service.max_upload_bytes, "file")?;
    let filename = query.into_inner().filename.or_else(|| {
        request
            .headers()
            .get(FILENAME_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    });
    let media = service.upload(caller.id.into(), bytes, filename).await?;
    Ok(HttpResponse::Created().json(media))
}

/// `GET /api/v1/media/{id}`: one of the caller's files, `200`.
#[utoipa::path(
    get,
    path = "/api/v1/media/{id}",
    tag = "media",
    operation_id = "metadata",
    params(("id" = uuid::Uuid, Path, description = "Media id")),
    responses(
        (status = 200, description = "File metadata", body = MediaResponse),
        (status = 400, description = "A malformed id", body = ErrorBody),
        (status = 401, description = "No valid bearer token", body = ErrorBody),
        (status = 404, description = "No such file, or it belongs to someone else", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn metadata(
    service: web::Data<MediaService>,
    caller: AuthUser,
    id: web::Path<uuid::Uuid>,
) -> Result<HttpResponse, AppError> {
    let media = service
        .metadata(caller.id.into(), id.into_inner().into())
        .await?;
    Ok(HttpResponse::Ok().json(media))
}

/// `DELETE /api/v1/media/{id}`: deletes one of the caller's files, `204`.
#[utoipa::path(
    delete,
    path = "/api/v1/media/{id}",
    tag = "media",
    operation_id = "remove",
    params(("id" = uuid::Uuid, Path, description = "Media id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 400, description = "A malformed id", body = ErrorBody),
        (status = 401, description = "No valid bearer token", body = ErrorBody),
        (status = 404, description = "No such file, or it belongs to someone else", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn remove(
    service: web::Data<MediaService>,
    caller: AuthUser,
    id: web::Path<uuid::Uuid>,
) -> Result<HttpResponse, AppError> {
    service
        .delete(caller.id.into(), id.into_inner().into())
        .await?;
    Ok(HttpResponse::NoContent().finish())
}

/// `GET /api/v1/media/{id}/content`: the bytes of one of the caller's
/// files, `200`, with the detected `Content-Type`. Only an image is shown
/// inline; anything else downloads, and `nosniff` and a sandboxing CSP
/// keep the browser from treating it as anything else.
#[utoipa::path(
    get,
    path = "/api/v1/media/{id}/content",
    tag = "media",
    operation_id = "content",
    params(("id" = uuid::Uuid, Path, description = "Media id")),
    responses(
        (status = 200, description = "The file's bytes, with the detected Content-Type"),
        (status = 400, description = "A malformed id", body = ErrorBody),
        (status = 401, description = "No valid bearer token", body = ErrorBody),
        (status = 404, description = "No such file, or it belongs to someone else", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn content(
    service: web::Data<MediaService>,
    caller: AuthUser,
    id: web::Path<uuid::Uuid>,
) -> Result<HttpResponse, AppError> {
    let content = service
        .content(caller.id.into(), id.into_inner().into())
        .await?;
    Ok(HttpResponse::Ok()
        .insert_header((header::CONTENT_TYPE, content.content_type))
        .insert_header((header::CONTENT_DISPOSITION, content.disposition))
        .insert_header((header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .insert_header((header::CONTENT_SECURITY_POLICY, SANDBOX))
        .body(content.bytes))
}
