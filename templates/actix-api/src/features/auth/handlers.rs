// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `auth` handlers: each parses the request, calls one
//! [`AuthService`] method, and answers. One function per endpoint.

use actix_web::{HttpResponse, web};

use super::dto::{CredentialsDto, LoginResponse, MessageResponse, UserResponse, VerifyEmailDto};
use super::service::AuthService;
use crate::error::AppError;
use crate::shared::auth::AuthUser;
use crate::shared::response::ErrorBody;
use crate::shared::validation::ValidatedJson;

/// `POST /api/v1/auth/register`: a new account, `201`.
#[utoipa::path(
    post,
    path = "/api/v1/auth/register",
    tag = "auth",
    operation_id = "register",
    request_body = CredentialsDto,
    responses(
        (status = 201, description = "Account created; a verification email was sent", body = UserResponse),
        (status = 400, description = "Password has appeared in a known breach (`breached_password`), or a malformed body", body = ErrorBody),
        (status = 409, description = "Email already registered", body = ErrorBody),
        (status = 422, description = "Not an email, or a password outside 8 to 128 characters", body = ErrorBody),
        (status = 429, description = "Too many requests from this IP", body = ErrorBody),
    ),
)]
pub async fn register(
    service: web::Data<AuthService>,
    body: ValidatedJson<CredentialsDto>,
) -> Result<HttpResponse, AppError> {
    let user = service.register(body.into_inner()).await?;
    Ok(HttpResponse::Created().json(UserResponse::from(user)))
}

/// `POST /api/v1/auth/login`: a session, `200`.
#[utoipa::path(
    post,
    path = "/api/v1/auth/login",
    tag = "auth",
    operation_id = "login",
    request_body = CredentialsDto,
    responses(
        (status = 200, description = "Either a session (access_token) or, for a two-factor-enabled account, requires_totp + pending_token", body = LoginResponse),
        (status = 400, description = "A malformed body", body = ErrorBody),
        (status = 401, description = "Invalid email/password, or the account is temporarily locked", body = ErrorBody),
        (status = 403, description = "Account disabled or email not verified", body = ErrorBody),
        (status = 422, description = "Not an email, or a password outside 8 to 128 characters", body = ErrorBody),
        (status = 429, description = "Too many requests from this IP", body = ErrorBody),
    ),
)]
pub async fn login(
    service: web::Data<AuthService>,
    body: ValidatedJson<CredentialsDto>,
) -> Result<HttpResponse, AppError> {
    let session = service.login(body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(LoginResponse::Session(session)))
}

/// `POST /api/v1/auth/email/verify`: verifies the email a link was mailed
/// to, `200`.
#[utoipa::path(
    post,
    path = "/api/v1/auth/email/verify",
    tag = "auth",
    operation_id = "verify",
    request_body = VerifyEmailDto,
    responses(
        (status = 200, description = "Email verified", body = MessageResponse),
        (status = 400, description = "Link is invalid, expired or already used (`invalid_token`), or a malformed body", body = ErrorBody),
        (status = 422, description = "An empty token", body = ErrorBody),
        (status = 429, description = "Too many requests from this IP", body = ErrorBody),
    ),
)]
pub async fn verify(
    service: web::Data<AuthService>,
    body: ValidatedJson<VerifyEmailDto>,
) -> Result<HttpResponse, AppError> {
    service.verify_email(&body.token).await?;
    Ok(HttpResponse::Ok().json(MessageResponse::new("email verified")))
}

/// `POST /api/v1/auth/email/verification/resend`: mails a new verification
/// link, `202`.
#[utoipa::path(
    post,
    path = "/api/v1/auth/email/verification/resend",
    tag = "auth",
    operation_id = "resend",
    responses(
        (status = 202, description = "Accepted; a no-op if already verified", body = MessageResponse),
        (status = 401, description = "No valid bearer token", body = ErrorBody),
        (status = 429, description = "Too many requests from this IP", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn resend(
    service: web::Data<AuthService>,
    caller: AuthUser,
) -> Result<HttpResponse, AppError> {
    service.resend_verification(caller.id.into()).await?;
    Ok(HttpResponse::Accepted().json(MessageResponse::new(
        "a new verification link is on its way, unless the email is already verified",
    )))
}
