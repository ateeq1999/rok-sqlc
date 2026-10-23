// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! API keys: long-lived credentials a user makes for scripts, sent as
//! `Authorization: Bearer rok_…` (told apart from a JWT by [`PREFIX`]) or
//! as `X-API-Key: rok_…`.
//!
//! A key is `rok_<8 characters>_<32 characters>`, base62. Only its first
//! characters (`key_prefix`, to recognise it in a list) and its SHA-256
//! (`key_hash`, to look a presented key up) are stored; the raw key is
//! answered once, when it is made. This module holds what both frameworks'
//! `shared/auth.rs` share: telling the credentials apart
//! ([`presented`]), checking a key ([`authenticate`]) and what a key may
//! reach ([`check_reach`]).

use chrono::{TimeDelta, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::token;
use crate::db::queries::session::{api_key_session, touch_api_key};
use crate::db::types::{ApiKeyScope, Role};
use crate::error::AppError;

/// What every key starts with.
pub const PREFIX: &str = "rok_";

/// The header a key may come in instead of `Authorization`.
pub const HEADER: &str = "x-api-key";

/// The most keys a user may have active (not revoked, not expired).
pub const MAX_ACTIVE: i64 = 25;

/// The paths no key reaches, whatever its scope: signing in, the account
/// flows, and managing keys need a session.
pub const SESSION_ONLY: &[&str] = &["/api/v1/auth", "/api/v1/api-keys"];

/// The characters of a key's random parts.
const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// How the caller proved who they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Credential {
    /// A session JWT, from signing in.
    Session,
    /// An API key with this scope.
    ApiKey(ApiKeyScope),
}

/// A new key: what the caller is shown once and what the database keeps.
pub struct NewApiKey {
    /// `rok_<8>_<32>`: answered once, never stored.
    pub raw: String,
    /// `rok_<8>`: stored as `key_prefix`, to recognise the key in a list.
    pub prefix: String,
    /// What `api_keys.key_hash` keeps: [`token::hash`] of `raw`.
    pub hash: String,
}

impl NewApiKey {
    /// A random key.
    #[must_use]
    pub fn generate() -> Self {
        let prefix = format!("{PREFIX}{}", base62(8));
        let raw = format!("{prefix}_{}", base62(32));
        let hash = token::hash(&raw);
        NewApiKey { raw, prefix, hash }
    }
}

/// `len` random base62 characters (rejection sampling, so each is equally
/// likely).
fn base62(len: usize) -> String {
    let mut out = String::with_capacity(len);
    while out.len() < len {
        let byte: u8 = rand::random();
        // 248 = 4 × 62: the bytes above it would favour the first characters.
        if byte < 248 {
            out.push(char::from(BASE62[usize::from(byte % 62)]));
        }
    }
    out
}

/// The credential a request presents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presented<'a> {
    /// A bearer token that isn't a key: a session JWT.
    Session(&'a str),
    /// An API key, from `Authorization: Bearer rok_…` or `X-API-Key`.
    ApiKey(&'a str),
}

/// The credential of a request whose `Authorization` and `X-API-Key`
/// headers are `authorization` and `api_key`. `Authorization` wins when
/// both are sent; neither is a `401`.
pub fn presented<'a>(
    authorization: Option<&'a str>,
    api_key: Option<&'a str>,
) -> Result<Presented<'a>, AppError> {
    match (
        authorization.and_then(|value| value.strip_prefix("Bearer ")),
        api_key,
    ) {
        (Some(token), _) if token.starts_with(PREFIX) => Ok(Presented::ApiKey(token)),
        (Some(token), _) => Ok(Presented::Session(token)),
        (None, Some(key)) => Ok(Presented::ApiKey(key)),
        (None, None) => Err(AppError::Unauthorized(
            "a bearer token is required".to_owned(),
        )),
    }
}

/// Whether a key with `scope` may call `method path`: no key reaches
/// [`SESSION_ONLY`], and a `read` key only `GET`, `HEAD` and `OPTIONS`.
/// Either refusal is a `403`.
///
/// `path` is compared [`normalised`]: decoded and resolved at least as far
/// as any router goes, so `/api/v1/api%2Dkeys` or `/api/v1//auth/me` is
/// refused like the plain path.
pub fn check_reach(scope: ApiKeyScope, method: &str, path: &str) -> Result<(), AppError> {
    let path = normalised(path);
    let session_only = SESSION_ONLY.iter().any(|root| {
        path.strip_prefix(root)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    });
    if session_only {
        return Err(AppError::Forbidden(
            "an API key can't reach this; sign in instead".to_owned(),
        ));
    }
    let safe = matches!(method, "GET" | "HEAD" | "OPTIONS");
    if scope == ApiKeyScope::Read && !safe {
        return Err(AppError::Forbidden(
            "a read API key may only GET, HEAD or OPTIONS".to_owned(),
        ));
    }
    Ok(())
}

/// `path` percent-decoded until nothing decodes further, its empty, `.`
/// and `..` segments resolved, in lower case: never less than what a
/// router decodes, so a route [`SESSION_ONLY`] covers can't be reached by
/// spelling it differently.
fn normalised(path: &str) -> String {
    let mut decoded = path.to_owned();
    loop {
        let next = percent_decoded(&decoded);
        if next == decoded {
            break;
        }
        decoded = next;
    }
    let mut segments: Vec<&str> = Vec::new();
    for segment in decoded.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            segment => segments.push(segment),
        }
    }
    format!("/{}", segments.join("/")).to_ascii_lowercase()
}

/// `text` with each `%XX` replaced by its byte (invalid UTF-8 replaced).
fn percent_decoded(text: &str) -> String {
    fn hex(byte: u8) -> Option<u8> {
        char::from(byte)
            .to_digit(16)
            .and_then(|digit| u8::try_from(digit).ok())
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escaped = match bytes.get(at..at + 3) {
            Some([b'%', high, low]) => hex(*high).zip(hex(*low)),
            _ => None,
        };
        match escaped {
            Some((high, low)) => {
                out.push((high << 4) | low);
                at += 3;
            }
            None => {
                out.push(bytes[at]);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Who a valid key belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyOwner {
    /// The owner's id.
    pub id: Uuid,
    /// The owner's role, from the database.
    pub role: Role,
    /// What the key may do.
    pub scope: ApiKeyScope,
}

/// The owner of the presented key `raw`, calling `method path`. An unknown,
/// revoked or expired key, or one whose owner is deleted or disabled, is a
/// `401`; a key that may not reach `method path` a `403` (see
/// [`check_reach`]). An admitted key's `last_used_at` is set, at most once
/// a minute.
pub async fn authenticate(
    pool: &PgPool,
    raw: &str,
    method: &str,
    path: &str,
) -> Result<KeyOwner, AppError> {
    let invalid = || AppError::Unauthorized("the API key is not valid".to_owned());
    let key = api_key_session(pool, &token::hash(raw))
        .await?
        .ok_or_else(invalid)?;
    let now = Utc::now();
    let expired = key.expires_at.is_some_and(|at| at <= now);
    if key.revoked_at.is_some() || expired || !key.is_active {
        return Err(invalid());
    }
    check_reach(key.scope, method, path)?;
    if key
        .last_used_at
        .is_none_or(|at| now - at >= TimeDelta::minutes(1))
    {
        touch_api_key(pool, key.id).await?;
    }
    Ok(KeyOwner {
        id: key.user_id.into(),
        role: key.role,
        scope: key.scope,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_rok_then_8_then_32_base62_characters_stored_as_its_hash() {
        let key = NewApiKey::generate();
        let parts: Vec<&str> = key.raw.split('_').collect();
        assert_eq!(parts.len(), 3, "{}", key.raw);
        assert_eq!(parts[0], "rok");
        assert_eq!(parts[1].len(), 8);
        assert_eq!(parts[2].len(), 32);
        assert!(
            key.raw[4..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "{}",
            key.raw
        );
        assert_eq!(key.prefix, key.raw[..12]);
        assert_eq!(key.hash, token::hash(&key.raw));
        assert_ne!(NewApiKey::generate().raw, key.raw);
    }

    #[test]
    fn a_bearer_key_is_told_apart_from_a_jwt_by_its_prefix() {
        assert_eq!(
            presented(Some("Bearer rok_abc"), None).ok(),
            Some(Presented::ApiKey("rok_abc"))
        );
        assert_eq!(
            presented(Some("Bearer eyJ.x.y"), Some("rok_abc")).ok(),
            Some(Presented::Session("eyJ.x.y"))
        );
        assert_eq!(
            presented(None, Some("rok_abc")).ok(),
            Some(Presented::ApiKey("rok_abc"))
        );
        assert!(matches!(
            presented(Some("Basic abc"), None),
            Err(AppError::Unauthorized(_))
        ));
        assert!(matches!(
            presented(None, None),
            Err(AppError::Unauthorized(_))
        ));
    }

    #[test]
    fn an_encoded_or_unnormalised_path_still_cannot_reach_auth_or_api_keys() {
        for path in [
            "/api/v1/api%2Dkeys",
            "/api/v1/api%2dkeys/0190",
            "/api/v1/%61uth/me",
            "/api/v1/%2561uth/me",
            "/api/v1//auth/me",
            "/api/v1/./auth/me",
            "/api/v1/users/../auth/me",
            "/api/v1/AUTH/me",
            "/api%2Fv1%2Fauth/me",
        ] {
            let refused = check_reach(ApiKeyScope::Write, "GET", path);
            assert!(matches!(refused, Err(AppError::Forbidden(_))), "{path}");
        }
    }

    #[test]
    fn a_read_key_may_only_use_safe_methods() {
        for method in ["GET", "HEAD", "OPTIONS"] {
            assert!(check_reach(ApiKeyScope::Read, method, "/api/v1/users/me").is_ok());
        }
        for method in ["POST", "PATCH", "PUT", "DELETE"] {
            let refused = check_reach(ApiKeyScope::Read, method, "/api/v1/users/me");
            assert!(matches!(refused, Err(AppError::Forbidden(_))), "{method}");
            assert!(check_reach(ApiKeyScope::Write, method, "/api/v1/users/me").is_ok());
        }
    }

    #[test]
    fn no_key_reaches_auth_or_api_keys() {
        for path in [
            "/api/v1/auth",
            "/api/v1/auth/me",
            "/api/v1/api-keys",
            "/api/v1/api-keys/0190",
        ] {
            let refused = check_reach(ApiKeyScope::Write, "GET", path);
            assert!(matches!(refused, Err(AppError::Forbidden(_))), "{path}");
        }
        assert!(check_reach(ApiKeyScope::Write, "GET", "/api/v1/authors").is_ok());
        assert!(check_reach(ApiKeyScope::Write, "GET", "/api/v1/%61uthors").is_ok());
        assert!(check_reach(ApiKeyScope::Write, "GET", "/api/v1/api-keys-x").is_ok());
    }
}
