// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! `AuthService`: the account flows' rules. Signing up, signing in (with
//! the lockout), and email verification. Every write goes through this
//! feature's query module, `db::queries::auth`; mail is enqueued on the
//! flow's own transaction and committed through the outbox.
//!
//! [`issue_token`] and [`consume_token`] are the single-use token lifecycle
//! every mailed link shares (verification, reset, email change,
//! invitation).

use std::sync::Arc;

use chrono::{TimeDelta, Utc};
use sqlx::{PgConnection, PgPool};

use super::dto::{
    ChangePasswordDto, CredentialsDto, ForgotPasswordDto, ResetPasswordDto, TokenResponse,
};
use crate::db::auth_tokens::models::AuthTokenId;
use crate::db::queries::auth::{self as queries, ConsumeTokenRow, InsertTokenParams};
use crate::db::types::AuthTokenPurpose;
use crate::db::users::models::{User, UserId};
use crate::db::users::read as users;
use crate::error::AppError;
use crate::shared::mail::{Outbox, Templates};
use crate::shared::security::breach::BreachCheck;
use crate::shared::security::jwt::JwtKeys;
use crate::shared::security::password;
use crate::shared::security::token::{self, SecretToken};

/// Wrong passwords in a row after which an account is locked.
pub const MAX_FAILED_LOGINS: i32 = 5;

/// How long a lock lasts, in minutes.
pub const LOCK_MINUTES: i32 = 15;

/// How long an email verification link stays valid, in hours.
pub const VERIFICATION_TTL_HOURS: i64 = 24;

/// The `401` of every failed sign-in: it doesn't say which part was wrong,
/// nor whether the account exists.
const BAD_CREDENTIALS: &str = "invalid email or password, or the account is temporarily locked";

/// The `400` of a link that can't be used.
const INVALID_TOKEN: &str = "the link is invalid, expired or already used";

/// The `auth` service, registered as app data by `app.rs`.
pub struct AuthService {
    /// The pool the flows read and write through.
    pub pool: PgPool,
    /// Signs the session tokens sign-in answers.
    pub keys: Arc<JwtKeys>,
    /// Where the flows' mail goes, on their transaction.
    pub outbox: Arc<Outbox>,
    /// The messages the flows send.
    pub templates: Arc<Templates>,
    /// Checks new passwords against known breaches (`BREACH_CHECK`).
    pub breach: Arc<dyn BreachCheck>,
    /// `REQUIRE_VERIFIED_EMAIL`: sign-in needs a verified email.
    pub require_verified_email: bool,
}

impl AuthService {
    /// Signs `input` up: a new `user` account with an unverified email,
    /// and a verification link mailed on the same transaction. A breached
    /// password is a `400 breached_password`; a live account with the
    /// same email is a `409`.
    pub async fn register(&self, input: CredentialsDto) -> Result<User, AppError> {
        self.reject_breached(&input.password).await?;
        let hash = password::hash(input.password).await?;
        let mut tx = self.pool.begin().await?;
        let user = queries::create_user(&mut *tx, UserId::generate(), &input.email, Some(&hash))
            .await
            .map_err(|err| match AppError::from(err) {
                AppError::Conflict(_) => {
                    AppError::Conflict("this email is already registered".to_owned())
                }
                other => other,
            })?;
        let link = issue_token(
            &mut tx,
            user.id,
            AuthTokenPurpose::EmailVerification,
            TimeDelta::hours(VERIFICATION_TTL_HOURS),
            None,
        )
        .await?;
        let mail = self.templates.verification(&user.email, &link.raw);
        self.outbox.enqueue(&mut tx, &mail).await?;
        self.outbox.commit(tx).await?;
        Ok(user)
    }

    /// Signs `input` in, answering a session token. A wrong password, an
    /// unknown email (checked against the dummy hash all the same) or a
    /// locked account is a `401`; [`MAX_FAILED_LOGINS`] wrong passwords in
    /// a row lock the account for [`LOCK_MINUTES`]. With the right
    /// password, a disabled account, or an unverified email when
    /// `require_verified_email`, is a `403`. Success forgets the failures.
    pub async fn login(&self, input: CredentialsDto) -> Result<TokenResponse, AppError> {
        let account = queries::find_login(&self.pool, &input.email).await?;
        let stored = account
            .as_ref()
            .and_then(|account| account.password_hash.clone());
        let matches = password::verify(input.password, stored).await?;
        let bad_credentials = || AppError::Unauthorized(BAD_CREDENTIALS.to_owned());
        let account = account.ok_or_else(bad_credentials)?;
        if account.locked_until.is_some_and(|until| until > Utc::now()) {
            return Err(bad_credentials());
        }
        if !matches {
            queries::record_failed_login(&self.pool, account.id, MAX_FAILED_LOGINS, LOCK_MINUTES)
                .await?;
            return Err(bad_credentials());
        }
        if !account.is_active {
            return Err(AppError::Forbidden("the account is disabled".to_owned()));
        }
        if self.require_verified_email && !account.email_verified {
            return Err(AppError::Forbidden(
                "the email address is not verified yet".to_owned(),
            ));
        }
        queries::clear_failed_logins(&self.pool, account.id).await?;
        let access_token =
            self.keys
                .issue(account.id.into(), account.role, account.token_version)?;
        Ok(TokenResponse {
            access_token,
            expires_in: i64::try_from(self.keys.ttl().as_secs()).unwrap_or(i64::MAX),
            token_type: "Bearer".to_owned(),
        })
    }

    /// Redeems an email verification link: the account's email is
    /// verified and the token spent. An unknown, expired or used token is
    /// a `400 invalid_token`.
    pub async fn verify_email(&self, raw: &str) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await?;
        let spent = consume_token(&mut tx, raw, AuthTokenPurpose::EmailVerification).await?;
        if queries::mark_email_verified(&mut *tx, spent.user_id).await? == 0 {
            return Err(AppError::InvalidToken(INVALID_TOKEN.to_owned()));
        }
        tx.commit().await?;
        Ok(())
    }

    /// Mails the account `id` a new verification link, spending the older
    /// ones; nothing when its email is already verified.
    pub async fn resend_verification(&self, id: UserId) -> Result<(), AppError> {
        let user = users::get_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::NotFound("no such account".to_owned()))?;
        if user.email_verified {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        let link = issue_token(
            &mut tx,
            user.id,
            AuthTokenPurpose::EmailVerification,
            TimeDelta::hours(VERIFICATION_TTL_HOURS),
            None,
        )
        .await?;
        let mail = self.templates.verification(&user.email, &link.raw);
        self.outbox.enqueue(&mut tx, &mail).await?;
        self.outbox.commit(tx).await
    }

    /// `400 breached_password` when the breach check knows `password`.
    pub async fn reject_breached(&self, password: &str) -> Result<(), AppError> {
        if self.breach.is_breached(password).await {
            return Err(AppError::BreachedPassword(
                "this password appears in a known data breach; choose another".to_owned(),
            ));
        }
        Ok(())
    }
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
        .ok_or_else(|| AppError::InvalidToken(INVALID_TOKEN.to_owned()))
}

/// How long a password reset link stays valid, in hours.
pub const PASSWORD_RESET_TTL_HOURS: i64 = 1;

/// The password flows: forgot, reset and change.
impl AuthService {
    /// Mails the live account with `email` a password reset link, spending
    /// its older ones. An unknown email gets nothing, and the caller can't
    /// tell the difference.
    pub async fn forgot_password(&self, input: ForgotPasswordDto) -> Result<(), AppError> {
        let Some(user) = users::get_by_email(&self.pool, &input.email).await? else {
            return Ok(());
        };
        let mut tx = self.pool.begin().await?;
        let link = issue_token(
            &mut tx,
            user.id,
            AuthTokenPurpose::PasswordReset,
            TimeDelta::hours(PASSWORD_RESET_TTL_HOURS),
            None,
        )
        .await?;
        let mail = self.templates.password_reset(&user.email, &link.raw);
        self.outbox.enqueue(&mut tx, &mail).await?;
        self.outbox.commit(tx).await
    }

    /// Redeems a password reset link, or an invitation's (whose email is
    /// then verified too): the account's password becomes `new_password`,
    /// every older session is signed out, any lock is lifted, and the
    /// token is spent. A breached password is a `400 breached_password`;
    /// an unknown, expired or used token a `400 invalid_token`.
    pub async fn reset_password(&self, input: ResetPasswordDto) -> Result<(), AppError> {
        self.reject_breached(&input.new_password).await?;
        let mut tx = self.pool.begin().await?;
        let (spent, invited) =
            match consume_token(&mut tx, &input.token, AuthTokenPurpose::PasswordReset).await {
                Ok(spent) => (spent, false),
                Err(AppError::InvalidToken(_)) => {
                    let spent =
                        consume_token(&mut tx, &input.token, AuthTokenPurpose::Invitation).await?;
                    (spent, true)
                }
                Err(other) => return Err(other),
            };
        let hash = password::hash(input.new_password).await?;
        if queries::reset_password(&mut *tx, spent.user_id, Some(&hash), invited).await? == 0 {
            return Err(AppError::InvalidToken(INVALID_TOKEN.to_owned()));
        }
        tx.commit().await?;
        Ok(())
    }

    /// Replaces the password of the account `id` when `current_password`
    /// is right (otherwise a `400 wrong_password`), signs every session
    /// out, the caller's included, and mails a notice. A breached password
    /// is a `400 breached_password`.
    pub async fn change_password(
        &self,
        id: UserId,
        input: ChangePasswordDto,
    ) -> Result<(), AppError> {
        let user = users::get_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::Unauthorized("the account no longer exists".to_owned()))?;
        if !password::verify(input.current_password, user.password_hash).await? {
            return Err(AppError::WrongPassword(
                "the current password is wrong".to_owned(),
            ));
        }
        self.reject_breached(&input.new_password).await?;
        let hash = password::hash(input.new_password).await?;
        let mut tx = self.pool.begin().await?;
        if queries::change_password(&mut *tx, user.id, Some(&hash)).await? == 0 {
            return Err(AppError::Unauthorized(
                "the account no longer exists".to_owned(),
            ));
        }
        let mail = self.templates.password_changed(&user.email);
        self.outbox.enqueue(&mut tx, &mail).await?;
        self.outbox.commit(tx).await
    }
}
