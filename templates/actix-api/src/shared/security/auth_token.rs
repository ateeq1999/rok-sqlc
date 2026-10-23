// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The single-use token lifecycle every mailed link shares (verification,
//! reset, email change, invitation), and a pending two-factor sign-in's
//! token too: what each purpose's token lasts and which message carries it
//! ([`policy`]), issuing one ([`issue_token`]) and spending it
//! ([`consume_token`]). Only a token's hash is stored (see
//! [`token`](super::token)).

use chrono::{TimeDelta, Utc};
use sqlx::PgConnection;

use super::token::{self, SecretToken};
use crate::db::auth_tokens::models::AuthTokenId;
use crate::db::queries::auth::{self as queries, ConsumeTokenRow, InsertTokenParams};
use crate::db::types::AuthTokenPurpose;
use crate::db::users::models::UserId;
use crate::error::AppError;
use crate::shared::mail::{Mail, Templates};

/// How long an email verification link stays valid, in hours.
pub const VERIFICATION_TTL_HOURS: i64 = 24;

/// How long a password reset link stays valid, in hours.
pub const PASSWORD_RESET_TTL_HOURS: i64 = 1;

/// How long an email change link stays valid, in hours.
pub const EMAIL_CHANGE_TTL_HOURS: i64 = 1;

/// How long an invitation link stays valid, in days.
pub const INVITATION_TTL_DAYS: i64 = 7;

/// How long a pending two-factor sign-in waits for its code, in minutes.
pub const TWO_FACTOR_PENDING_TTL_MINUTES: i64 = 5;

/// The `400` of a link that can't be used.
const INVALID_TOKEN: &str = "the link is invalid, expired or already used";

/// Renders the message carrying a link: `(templates, to, raw token)`.
pub type RenderLink = fn(&Templates, &str, &str) -> Mail;

/// How long a `purpose` token lasts, and the message that carries it: none
/// for a pending two-factor sign-in, whose token sign-in answers instead.
#[must_use]
pub fn policy(purpose: AuthTokenPurpose) -> (TimeDelta, Option<RenderLink>) {
    match purpose {
        AuthTokenPurpose::EmailVerification => (
            TimeDelta::hours(VERIFICATION_TTL_HOURS),
            Some(Templates::verification),
        ),
        AuthTokenPurpose::PasswordReset => (
            TimeDelta::hours(PASSWORD_RESET_TTL_HOURS),
            Some(Templates::password_reset),
        ),
        AuthTokenPurpose::EmailChange => (
            TimeDelta::hours(EMAIL_CHANGE_TTL_HOURS),
            Some(Templates::email_change),
        ),
        AuthTokenPurpose::Invitation => (
            TimeDelta::days(INVITATION_TTL_DAYS),
            Some(Templates::invitation),
        ),
        AuthTokenPurpose::TwoFactorPending => {
            (TimeDelta::minutes(TWO_FACTOR_PENDING_TTL_MINUTES), None)
        }
    }
}

/// The `400 invalid_token` of a link that can't be used.
#[must_use]
pub fn invalid_token() -> AppError {
    AppError::InvalidToken(INVALID_TOKEN.to_owned())
}

/// Issues a single-use token for `user` and `purpose`, valid for `ttl`, on
/// `conn` (the flow's transaction): the unused tokens it replaces are
/// spent, and only its hash is stored. Mail the returned `raw` in a link.
pub async fn issue_token(
    conn: &mut PgConnection,
    user: UserId,
    purpose: AuthTokenPurpose,
    ttl: TimeDelta,
    new_email: Option<String>,
) -> Result<SecretToken, AppError> {
    let secret = SecretToken::generate();
    queries::revoke_tokens(&mut *conn, user, purpose).await?;
    queries::insert_token(
        &mut *conn,
        InsertTokenParams {
            id: AuthTokenId::generate(),
            user_id: user,
            purpose,
            token_hash: secret.hash.clone(),
            new_email,
            expires_at: Utc::now() + ttl,
        },
    )
    .await?;
    Ok(secret)
}

/// Spends the token `raw` on `conn`, if it is unused, unexpired and was
/// issued for `purpose`, and answers whom it was for (and an email
/// change's new address). Anything else is a `400 invalid_token`.
pub async fn consume_token(
    conn: &mut PgConnection,
    raw: &str,
    purpose: AuthTokenPurpose,
) -> Result<ConsumeTokenRow, AppError> {
    queries::consume_token(conn, &token::hash(raw), purpose)
        .await?
        .ok_or_else(invalid_token)
}
