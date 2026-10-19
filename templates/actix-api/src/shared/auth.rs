// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Who is calling: the [`AuthUser`] and [`AdminUser`] extractors. Each
//! verifies an HS256 JWT from `Authorization: Bearer <token>` against the
//! [`JwtKeys`] that `app.rs` registers as app data. No valid token is a
//! `401`, and a caller who is not an admin is a `403` for `AdminUser`, both
//! through [`AppError`].

use std::future::{Ready, ready};

use actix_web::dev::Payload;
use actix_web::http::header;
use actix_web::{FromRequest, HttpRequest, web};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;

/// A caller's role, `"user"` or `"admin"` in a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Any signed-in caller.
    User,
    /// An administrator.
    Admin,
}

/// A caller with a valid token. Take it in a handler to require one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthUser {
    /// The token's `sub`.
    pub id: Uuid,
    /// The token's `role`.
    pub role: Role,
}

/// A caller with a valid token and the `admin` role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdminUser(pub AuthUser);

/// The claims of a token: who, which role, and until when (`exp`, seconds
/// since the Unix epoch).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// The caller's id.
    pub sub: Uuid,
    /// The caller's role.
    pub role: Role,
    /// When the token expires.
    pub exp: u64,
}

/// The key tokens are verified with, from `JWT_SECRET`.
pub struct JwtKeys {
    decoding: DecodingKey,
}

impl JwtKeys {
    /// The keys for the HS256 `secret`.
    #[must_use]
    pub fn new(secret: &str) -> Self {
        JwtKeys {
            decoding: DecodingKey::from_secret(secret.as_bytes()),
        }
    }
}

impl FromRequest for AuthUser {
    type Error = AppError;
    type Future = Ready<Result<Self, AppError>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        ready(authenticate(req))
    }
}

impl FromRequest for AdminUser {
    type Error = AppError;
    type Future = Ready<Result<Self, AppError>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        ready(authenticate(req).and_then(|user| match user.role {
            Role::Admin => Ok(AdminUser(user)),
            Role::User => Err(AppError::Forbidden("this needs the admin role".to_owned())),
        }))
    }
}

/// The caller of `req`, from its bearer token.
fn authenticate(req: &HttpRequest) -> Result<AuthUser, AppError> {
    let keys = req
        .app_data::<web::Data<JwtKeys>>()
        .ok_or_else(|| AppError::Internal("`JwtKeys` is not registered as app data".into()))?;
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::Unauthorized("a bearer token is required".to_owned()))?;
    let token = decode::<Claims>(token, &keys.decoding, &Validation::new(Algorithm::HS256))
        .map_err(|_| AppError::Unauthorized("the token is invalid or expired".to_owned()))?;
    Ok(AuthUser {
        id: token.claims.sub,
        role: token.claims.role,
    })
}
