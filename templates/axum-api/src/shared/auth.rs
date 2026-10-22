// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Who is calling: the [`AuthUser`] and [`AdminUser`] extractors.
//!
//! Each verifies the HS256 JWT from `Authorization: Bearer <token>` with the
//! [`JwtKeys`] of the state (`app.rs`'s `AppState`), then loads the account
//! with one query (`db::queries::session::session_state`, on the state's
//! `PgPool`): its `token_version`, whether it is active, and its role. A
//! missing or invalid token, a soft-deleted or disabled account, or a token
//! issued before the last `token_version` bump is a `401`. The role comes
//! from the database, not the token, so a promotion or demotion applies on
//! the next request; a caller who is not an admin is a `403` for
//! `AdminUser`. Both answer through [`AppError`].
//!
//! An API key stands in for the JWT: `Authorization: Bearer rok_…` (a key
//! is told apart by its prefix) or `X-API-Key`. It is checked by
//! [`api_key::authenticate`]: unknown, revoked or expired keys, and keys of
//! a deleted or disabled account, are a `401`; a `read` key on a method
//! that isn't safe, and any key on `/api/v1/auth` or `/api/v1/api-keys`, a
//! `403`. [`AuthUser::credential`] says which one the caller sent.

use std::sync::Arc;

use axum::extract::{FromRef, FromRequestParts, OriginalUri};
use axum::http::request::Parts;
use axum::http::{HeaderMap, header};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::queries::session::{SessionStateRow, session_state};
pub use crate::db::types::Role;
use crate::error::AppError;
pub use crate::shared::security::api_key::Credential;
use crate::shared::security::api_key::{self, Presented};
pub use crate::shared::security::jwt::{Claims, JwtKeys};

/// A signed-in caller with a live, active account. Take it in a handler to
/// require one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthUser {
    /// The token's `sub`.
    pub id: Uuid,
    /// The account's role, from the database.
    pub role: Role,
    /// How the caller proved who they are: a session or an API key.
    pub credential: Credential,
}

/// A signed-in caller whose account has the `admin` role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdminUser(pub AuthUser);

impl<S> FromRequestParts<S> for AuthUser
where
    Arc<JwtKeys>: FromRef<S>,
    PgPool: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, AppError> {
        let pool = PgPool::from_ref(state);
        match credentials(&parts.headers)? {
            Presented::Session(token) => {
                let claims = Arc::<JwtKeys>::from_ref(state).decode(token)?;
                let account = session_state(&pool, claims.sub.into()).await?;
                admit(&claims, account)
            }
            Presented::ApiKey(key) => {
                // A nested router sees its own part of the path; the rules
                // name whole paths.
                let path = parts
                    .extensions
                    .get::<OriginalUri>()
                    .map_or_else(|| parts.uri.path(), |original| original.path());
                let owner = api_key::authenticate(&pool, key, parts.method.as_str(), path).await?;
                Ok(AuthUser {
                    id: owner.id,
                    role: owner.role,
                    credential: Credential::ApiKey(owner.scope),
                })
            }
        }
    }
}

impl<S> FromRequestParts<S> for AdminUser
where
    Arc<JwtKeys>: FromRef<S>,
    PgPool: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, AppError> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        match user.role {
            Role::Admin => Ok(AdminUser(user)),
            Role::User => Err(AppError::Forbidden("this needs the admin role".to_owned())),
        }
    }
}

/// The credential `headers` present: a bearer token, or an API key.
fn credentials(headers: &HeaderMap) -> Result<Presented<'_>, AppError> {
    let value = |name| headers.get(name).and_then(|value| value.to_str().ok());
    api_key::presented(
        value(header::AUTHORIZATION.as_str()),
        value(api_key::HEADER),
    )
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
        credential: Credential::Session,
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
                role: Role::Admin,
                credential: Credential::Session,
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
