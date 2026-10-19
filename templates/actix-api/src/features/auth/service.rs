// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! `AuthService`: the account flows' rules. Signing up, signing in (with
//! the lockout), email verification, the password flows, email change and
//! invitations. Every write goes through this feature's query module,
//! `db::queries::auth`, or through a port; mail is enqueued on the flow's
//! own transaction and committed through the outbox.
//!
//! [`issue_token`] and [`consume_token`] are the single-use token lifecycle
//! every mailed link shares (verification, reset, email change,
//! invitation).

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{TimeDelta, Utc};
use sqlx::{PgConnection, PgPool};

use super::dto::{
    ChangeEmailDto, ChangePasswordDto, CredentialsDto, ForgotPasswordDto, InviteUserDto,
    ResetPasswordDto, TokenResponse,
};
use crate::db::auth_tokens::models::AuthTokenId;
use crate::db::queries::auth::{
    self as queries, ConsumeTokenRow, InsertTokenParams, InviteUserParams,
};
use crate::db::types::{AuthTokenPurpose, Role};
use crate::db::users::models::{User, UserId};
use crate::db::users::read as users;
use crate::error::AppError;
use crate::shared::mail::{Mail, Outbox, Templates};
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

/// How long a password reset link stays valid, in hours.
pub const PASSWORD_RESET_TTL_HOURS: i64 = 1;

/// How long an email change link stays valid, in hours.
pub const EMAIL_CHANGE_TTL_HOURS: i64 = 1;

/// How long an invitation link stays valid, in days.
pub const INVITATION_TTL_DAYS: i64 = 7;

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
    /// Records what admins do in the audit log (the `audit_log` port).
    pub audit: Arc<dyn AuditTrail>,
}

/// Signing up, signing in and email verification.
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
            .map_err(conflict_as(email_taken))?;
        self.mail_link(
            &mut tx,
            user.id,
            &user.email,
            AuthTokenPurpose::EmailVerification,
            None,
        )
        .await?;
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
            return Err(invalid_token());
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
        self.mail_link(
            &mut tx,
            user.id,
            &user.email,
            AuthTokenPurpose::EmailVerification,
            None,
        )
        .await?;
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

    /// Issues a `purpose` link for `user` on `conn` (the flow's
    /// transaction), spending the older ones, and enqueues the message that
    /// carries it to `to`. The caller commits through the outbox.
    async fn mail_link(
        &self,
        conn: &mut PgConnection,
        user: UserId,
        to: &str,
        purpose: AuthTokenPurpose,
        new_email: Option<String>,
    ) -> Result<(), AppError> {
        let (ttl, render) = link_kind(purpose);
        let link = issue_token(&mut *conn, user, purpose, ttl, new_email).await?;
        let mail = render(&self.templates, to, &link.raw);
        self.outbox.enqueue(conn, &mail).await
    }
}

/// Renders the message carrying a link: `(templates, to, raw token)`.
type RenderLink = fn(&Templates, &str, &str) -> Mail;

/// How long a `purpose` link lasts, and the message that carries it.
fn link_kind(purpose: AuthTokenPurpose) -> (TimeDelta, RenderLink) {
    match purpose {
        AuthTokenPurpose::EmailVerification => (
            TimeDelta::hours(VERIFICATION_TTL_HOURS),
            Templates::verification,
        ),
        AuthTokenPurpose::PasswordReset => (
            TimeDelta::hours(PASSWORD_RESET_TTL_HOURS),
            Templates::password_reset,
        ),
        AuthTokenPurpose::EmailChange => (
            TimeDelta::hours(EMAIL_CHANGE_TTL_HOURS),
            Templates::email_change,
        ),
        AuthTokenPurpose::Invitation => {
            (TimeDelta::days(INVITATION_TTL_DAYS), Templates::invitation)
        }
    }
}

/// The `400 invalid_token` of a link that can't be used.
fn invalid_token() -> AppError {
    AppError::InvalidToken(INVALID_TOKEN.to_owned())
}

/// The `409` of an email a live account already uses.
fn email_taken() -> AppError {
    AppError::Conflict("this email is already registered".to_owned())
}

/// Maps an error to [`AppError`], answering `replacement` instead for a
/// unique violation (a `409`): what a taken email means to the flow.
fn conflict_as<E: Into<AppError>>(replacement: fn() -> AppError) -> impl FnOnce(E) -> AppError {
    move |err| match err.into() {
        AppError::Conflict(_) => replacement(),
        other => other,
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
        .ok_or_else(invalid_token)
}

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
        self.mail_link(
            &mut tx,
            user.id,
            &user.email,
            AuthTokenPurpose::PasswordReset,
            None,
        )
        .await?;
        self.outbox.commit(tx).await
    }

    /// Redeems a password reset link, or an invitation's (whose email is
    /// then verified too): the account's password becomes `new_password`,
    /// every older session is signed out, any lock is lifted, and the
    /// token is spent.
    ///
    /// The link is checked first, without spending it: an unknown, expired
    /// or used token is a `400 invalid_token` that costs no breach check
    /// and no hash. Then a breached password is a `400 breached_password`
    /// (the link still works). The hash is computed before the transaction
    /// opens, which then spends the token and writes the password.
    pub async fn reset_password(&self, input: ResetPasswordDto) -> Result<(), AppError> {
        let purpose = queries::live_token_purpose(&self.pool, &token::hash(&input.token))
            .await?
            .filter(|purpose| {
                matches!(
                    purpose,
                    AuthTokenPurpose::PasswordReset | AuthTokenPurpose::Invitation
                )
            })
            .ok_or_else(invalid_token)?;
        self.reject_breached(&input.new_password).await?;
        let hash = password::hash(input.new_password).await?;
        let invited = purpose == AuthTokenPurpose::Invitation;
        let mut tx = self.pool.begin().await?;
        let spent = consume_token(&mut tx, &input.token, purpose).await?;
        if queries::reset_password(&mut *tx, spent.user_id, Some(&hash), invited).await? == 0 {
            return Err(invalid_token());
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
            return Err(wrong_password());
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

/// The `400 wrong_password` of a wrong current password.
fn wrong_password() -> AppError {
    AppError::WrongPassword("the current password is wrong".to_owned())
}

/// Email change: request and confirm.
impl AuthService {
    /// Starts moving the account `id` to `input.new_email`. With the right
    /// current password (otherwise a `400 wrong_password`), a notice goes
    /// to the old address and, when no live account uses the new one, a
    /// confirmation link to it, on one transaction, spending older links.
    /// A taken address gets nothing, but the notice goes all the same and
    /// the answer is the same, so neither tells who has an account.
    pub async fn request_email_change(
        &self,
        id: UserId,
        input: ChangeEmailDto,
    ) -> Result<(), AppError> {
        let user = users::get_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::NotFound("no such account".to_owned()))?;
        if !password::verify(input.current_password, user.password_hash).await? {
            return Err(wrong_password());
        }
        let taken = queries::find_login(&self.pool, &input.new_email)
            .await?
            .is_some();
        let mut tx = self.pool.begin().await?;
        let notice = self
            .templates
            .email_change_notice(&user.email, &input.new_email);
        self.outbox.enqueue(&mut tx, &notice).await?;
        if !taken {
            let new_email = input.new_email;
            self.mail_link(
                &mut tx,
                user.id,
                &new_email,
                AuthTokenPurpose::EmailChange,
                Some(new_email.clone()),
            )
            .await?;
        }
        self.outbox.commit(tx).await
    }

    /// Redeems an email change link: the account's email becomes the new
    /// address, verified, and the token is spent. A bad, expired or used
    /// token, or an address a live account took meanwhile (even in a race
    /// with this request), is a `400 invalid_token`.
    pub async fn confirm_email_change(&self, raw: &str) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await?;
        let spent = consume_token(&mut tx, raw, AuthTokenPurpose::EmailChange).await?;
        let new_email = spent.new_email.ok_or_else(invalid_token)?;
        let changed = queries::change_email(&mut *tx, spent.user_id, &new_email)
            .await
            .map_err(conflict_as(invalid_token))?;
        if changed == 0 {
            return Err(invalid_token());
        }
        tx.commit().await?;
        Ok(())
    }
}

/// One entry for the audit log: who did what to whom.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditEntry {
    /// Who acted.
    pub actor: UserId,
    /// Whose account it was done to, if anyone's.
    pub target: Option<UserId>,
    /// What was done, such as `user.invited`.
    pub action: String,
}

/// The port onto the `audit_log` feature: `auth` records what admins do
/// through it, on the flow's own connection, so the entry commits (or rolls
/// back) with the change it records. `app.rs` implements it.
#[async_trait]
pub trait AuditTrail: Send + Sync {
    /// Records `entry` on `conn`.
    async fn record_in(&self, conn: &mut PgConnection, entry: AuditEntry) -> Result<(), AppError>;
}

/// Invitations.
impl AuthService {
    /// Invites someone on behalf of the admin `actor`: an account with
    /// `input`'s email, display name and role (`user` by default), no
    /// password and an unverified email; a `user.invited` audit entry; and
    /// an `invitation` link mailed to it (redeemed through the password
    /// reset), all on one transaction. A live account with the same email
    /// is a `409`.
    pub async fn invite(&self, actor: UserId, input: InviteUserDto) -> Result<User, AppError> {
        let mut tx = self.pool.begin().await?;
        let params = InviteUserParams {
            id: UserId::generate(),
            email: input.email,
            display_name: input.display_name,
            role: input.role.unwrap_or(Role::User),
        };
        let user = queries::invite_user(&mut *tx, params)
            .await
            .map_err(conflict_as(email_taken))?;
        self.audit
            .record_in(
                &mut tx,
                AuditEntry {
                    actor,
                    target: Some(user.id),
                    action: "user.invited".to_owned(),
                },
            )
            .await?;
        self.mail_link(
            &mut tx,
            user.id,
            &user.email,
            AuthTokenPurpose::Invitation,
            None,
        )
        .await?;
        self.outbox.commit(tx).await?;
        Ok(user)
    }
}
