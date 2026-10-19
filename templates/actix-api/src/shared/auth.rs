// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Who is calling: the [`AuthUser`] and [`AdminUser`] extractors.
//!
//! Each verifies the HS256 JWT from `Authorization: Bearer <token>` with the
//! [`JwtKeys`] that `app.rs` registers as app data, then loads the account
//! with one query (`db::queries::session::session_state`, on the
//! `web::Data<PgPool>` app data): its `token_version`, whether it is active,
//! and its role. A missing or invalid token, a soft-deleted or disabled
//! account, or a token issued before the last `token_version` bump is a
//! `401`. The role comes from the database, not the token, so a promotion
//! or demotion applies on the next request; a caller who is not an admin is
//! a `403` for `AdminUser`. Both answer through [`AppError`].

use std::future::Future;
use std::pin::Pin;

use actix_web::dev::Payload;
use actix_web::http::header;
use actix_web::{FromRequest, HttpRequest, web};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::queries::session::{SessionStateRow, session_state};
pub use crate::db::types::Role;
use crate::error::AppError;
pub use crate::shared::security::jwt::{Claims, JwtKeys};

/// A signed-in caller with a live, active account. Take it in a handler to
/// require one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthUser {
    /// The token's `sub`.
    pub id: Uuid,
    /// The account's role, from the database.
    pub role: Role,
}

/// A signed-in caller whose account has the `admin` role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdminUser(pub AuthUser);

impl FromRequest for AuthUser {
    type Error = AppError;
    type Future = Pin<Box<dyn Future<Output = Result<Self, AppError>>>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        let checked = credentials(req);
        Box::pin(async move {
            let (claims, pool) = checked?;
            let state = session_state(pool.get_ref(), claims.sub.into()).await?;
            admit(&claims, state)
        })
    }
}

impl FromRequest for AdminUser {
    type Error = AppError;
    type Future = Pin<Box<dyn Future<Output = Result<Self, AppError>>>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let user = AuthUser::from_request(req, payload);
        Box::pin(async move {
            let user = user.await?;
            match user.role {
                Role::Admin => Ok(AdminUser(user)),
                Role::User => Err(AppError::Forbidden("this needs the admin role".to_owned())),
            }
        })
    }
}

/// The verified claims of `req`'s bearer token, and the pool to check the
/// account with.
fn credentials(req: &HttpRequest) -> Result<(Claims, web::Data<PgPool>), AppError> {
    let keys = req
        .app_data::<web::Data<JwtKeys>>()
        .ok_or_else(|| AppError::Internal("`JwtKeys` is not registered as app data".into()))?;
    let pool = req
        .app_data::<web::Data<PgPool>>()
        .ok_or_else(|| AppError::Internal("`PgPool` is not registered as app data".into()))?;
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::Unauthorized("a bearer token is required".to_owned()))?;
    Ok((keys.decode(token)?, pool.clone()))
}

/// The caller of a valid token, given its account's `state` (`None` for no
/// live account).
fn admit(claims: &Claims, state: Option<SessionStateRow>) -> Result<AuthUser, AppError> {
    let revoked = || AppError::Unauthorized("the token is no longer valid".to_owned());
    let state = state.ok_or_else(revoked)?;
    if !state.is_active || state.token_version != claims.tv {
        return Err(revoked());
    }
    Ok(AuthUser {
        id: claims.sub,
        role: state.role,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(tv: i32) -> Claims {
        Claims {
            sub: Uuid::now_v7(),
            role: Role::User,
            tv,
            exp: u64::MAX,
        }
    }

    fn state(token_version: i32, is_active: bool, role: Role) -> Option<SessionStateRow> {
        Some(SessionStateRow {
            token_version,
            is_active,
            role,
        })
    }

    #[test]
    fn a_live_account_is_admitted_with_its_role_from_the_database() -> Result<(), AppError> {
        let claims = claims(2);
        let user = admit(&claims, state(2, true, Role::Admin))?;
        assert_eq!(
            user,
            AuthUser {
                id: claims.sub,
                role: Role::Admin
            }
        );
        Ok(())
    }

    #[test]
    fn a_stale_token_version_is_rejected() {
        let rejected = admit(&claims(1), state(2, true, Role::User));
        assert!(
            matches!(rejected, Err(AppError::Unauthorized(_))),
            "{rejected:?}"
        );
    }

    #[test]
    fn a_disabled_or_missing_account_is_rejected() {
        let rejected = admit(&claims(1), state(1, false, Role::User));
        assert!(
            matches!(rejected, Err(AppError::Unauthorized(_))),
            "{rejected:?}"
        );
        let rejected = admit(&claims(1), None);
        assert!(
            matches!(rejected, Err(AppError::Unauthorized(_))),
            "{rejected:?}"
        );
    }
}
