// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `two_factor` handlers: each parses the request, calls one
//! [`TwoFactorService`] method, and answers. One function per endpoint.

use actix_web::{HttpResponse, web};

use super::dto::{
    DisableTotpDto, EnableTotpDto, MessageResponse, TokenResponse, TotpEnabledResponse,
    TotpSetupResponse, VerifyTotpDto,
};
use super::service::TwoFactorService;
use crate::error::AppError;
use crate::shared::auth::AuthUser;
use crate::shared::response::ErrorBody;
use crate::shared::validation::ValidatedJson;

/// `POST /api/v1/auth/2fa/setup`: a new authenticator secret, not enforced
/// until enabled, `200`.
#[utoipa::path(
    post,
    path = "/api/v1/auth/2fa/setup",
    tag = "auth",
    operation_id = "setup",
    responses(
        (status = 200, description = "Secret + QR code; not yet enforced until /2fa/enable confirms it", body = TotpSetupResponse),
        (status = 400, description = "Two-factor is already on", body = ErrorBody),
        (status = 401, description = "No valid bearer token", body = ErrorBody),
        (status = 429, description = "Too many requests from this IP", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn setup_totp(
    service: web::Data<TwoFactorService>,
    caller: AuthUser,
) -> Result<HttpResponse, AppError> {
    let setup = service.setup_totp(caller.id.into()).await?;
    Ok(HttpResponse::Ok().json(setup))
}

/// `POST /api/v1/auth/2fa/enable`: two-factor on, with recovery codes,
/// `200`.
#[utoipa::path(
    post,
    path = "/api/v1/auth/2fa/enable",
    tag = "auth",
    operation_id = "enable",
    request_body = EnableTotpDto,
    responses(
        (status = 200, description = "Two-factor enabled; recovery codes shown exactly once", body = TotpEnabledResponse),
        (status = 400, description = "Wrong code, already enabled, or no setup in progress", body = ErrorBody),
        (status = 401, description = "No valid bearer token", body = ErrorBody),
        (status = 422, description = "An empty code, or one over 32 characters", body = ErrorBody),
        (status = 429, description = "Too many requests from this IP", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn enable_totp(
    service: web::Data<TwoFactorService>,
    caller: AuthUser,
    body: ValidatedJson<EnableTotpDto>,
) -> Result<HttpResponse, AppError> {
    let enabled = service
        .enable_totp(caller.id.into(), body.into_inner())
        .await?;
    Ok(HttpResponse::Ok().json(enabled))
}

/// `POST /api/v1/auth/2fa/disable`: two-factor off, `200`.
#[utoipa::path(
    post,
    path = "/api/v1/auth/2fa/disable",
    tag = "auth",
    operation_id = "disable",
    request_body = DisableTotpDto,
    responses(
        (status = 200, description = "Two-factor disabled", body = MessageResponse),
        (status = 400, description = "Wrong current password (`wrong_password`), or a malformed body", body = ErrorBody),
        (status = 401, description = "No valid bearer token", body = ErrorBody),
        (status = 429, description = "Too many requests from this IP", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn disable_totp(
    service: web::Data<TwoFactorService>,
    caller: AuthUser,
    body: ValidatedJson<DisableTotpDto>,
) -> Result<HttpResponse, AppError> {
    service
        .disable_totp(caller.id.into(), body.into_inner())
        .await?;
    Ok(HttpResponse::Ok().json(MessageResponse::new("two-factor sign-in disabled")))
}

/// `POST /api/v1/auth/2fa/verify`: a sign-in's second step, a session,
/// `200`.
#[utoipa::path(
    post,
    path = "/api/v1/auth/2fa/verify",
    tag = "auth",
    operation_id = "verify",
    request_body = VerifyTotpDto,
    responses(
        (status = 200, description = "Completes the login", body = TokenResponse),
        (status = 400, description = "pending_token is invalid or expired (`invalid_token`), or a malformed body", body = ErrorBody),
        (status = 401, description = "Wrong authenticator/recovery code, or the account is temporarily locked", body = ErrorBody),
        (status = 403, description = "Account disabled", body = ErrorBody),
        (status = 422, description = "An empty token or code, or a code over 32 characters", body = ErrorBody),
        (status = 429, description = "Too many requests from this IP", body = ErrorBody),
    ),
)]
pub async fn verify_totp(
    service: web::Data<TwoFactorService>,
    body: ValidatedJson<VerifyTotpDto>,
) -> Result<HttpResponse, AppError> {
    let session = service.verify_totp(body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(session))
}
