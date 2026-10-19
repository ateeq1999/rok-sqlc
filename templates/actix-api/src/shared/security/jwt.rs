// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The session: an access-only HS256 JWT carrying `sub` (the user's id),
//! `role`, `tv` (the user's `token_version` when it was issued) and `exp`.
//! There is no refresh token; a password change, a reset or an admin
//! disable bumps `users.token_version`, and `shared/auth.rs` rejects a
//! token whose `tv` no longer matches.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::types::Role;
use crate::error::AppError;

/// The claims of a session token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// The user's id.
    pub sub: Uuid,
    /// The user's role when the token was issued. `AdminUser` reads the
    /// role from the database instead, so a change applies at once.
    pub role: Role,
    /// The user's `token_version` when the token was issued.
    pub tv: i32,
    /// When the token expires, in seconds since the Unix epoch.
    pub exp: u64,
}

/// The keys tokens are signed and verified with, from `JWT_SECRET`, and
/// how long an issued token lasts (`JWT_TTL`). `app.rs` registers them as
/// app data.
pub struct JwtKeys {
    encoding: EncodingKey,
    decoding: DecodingKey,
    ttl: Duration,
}

impl JwtKeys {
    /// The keys for the HS256 `secret`, issuing tokens valid for `ttl`.
    #[must_use]
    pub fn new(secret: &str, ttl: Duration) -> Self {
        JwtKeys {
            encoding: EncodingKey::from_secret(secret.as_bytes()),
            decoding: DecodingKey::from_secret(secret.as_bytes()),
            ttl,
        }
    }

    /// How long an issued token lasts: `TokenResponse.expires_in`.
    #[must_use]
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// A token for the user `id` with `role` and `token_version`, expiring
    /// [`ttl`](Self::ttl) from now.
    pub fn issue(&self, id: Uuid, role: Role, token_version: i32) -> Result<String, AppError> {
        let claims = Claims {
            sub: id,
            role,
            tv: token_version,
            exp: now()? + self.ttl.as_secs(),
        };
        jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &self.encoding)
            .map_err(|err| AppError::Internal(Box::new(err)))
    }

    /// The claims of `token`, when it is signed with these keys and not
    /// expired; otherwise a `401`.
    pub fn decode(&self, token: &str) -> Result<Claims, AppError> {
        let mut validation = Validation::new(Algorithm::HS256);
        // The server checks its own tokens: no clock skew to allow for.
        validation.leeway = 0;
        jsonwebtoken::decode::<Claims>(token, &self.decoding, &validation)
            .map(|data| data.claims)
            .map_err(|_| AppError::Unauthorized("the token is invalid or expired".to_owned()))
    }
}

/// Seconds since the Unix epoch.
fn now() -> Result<u64, AppError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .map_err(|err| AppError::Internal(Box::new(err)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "a test secret";

    #[test]
    fn a_token_round_trips_its_subject_role_and_version() -> Result<(), AppError> {
        let keys = JwtKeys::new(SECRET, Duration::from_secs(3600));
        let id = Uuid::now_v7();
        let claims = keys.decode(&keys.issue(id, Role::Admin, 3)?)?;
        assert_eq!(claims.sub, id);
        assert_eq!(claims.role, Role::Admin);
        assert_eq!(claims.tv, 3);
        assert_eq!(keys.ttl(), Duration::from_secs(3600));
        Ok(())
    }

    #[test]
    fn a_token_signed_with_another_secret_is_rejected() -> Result<(), AppError> {
        let theirs = JwtKeys::new("another secret", Duration::from_secs(3600));
        let token = theirs.issue(Uuid::now_v7(), Role::User, 1)?;
        let rejected = JwtKeys::new(SECRET, Duration::from_secs(3600)).decode(&token);
        assert!(
            matches!(rejected, Err(AppError::Unauthorized(_))),
            "{rejected:?}"
        );
        Ok(())
    }

    #[test]
    fn an_expired_token_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let claims = Claims {
            sub: Uuid::now_v7(),
            role: Role::User,
            tv: 1,
            exp: now()? - 1,
        };
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(SECRET.as_bytes()),
        )?;
        let rejected = JwtKeys::new(SECRET, Duration::from_secs(3600)).decode(&token);
        assert!(
            matches!(rejected, Err(AppError::Unauthorized(_))),
            "{rejected:?}"
        );
        Ok(())
    }
}
