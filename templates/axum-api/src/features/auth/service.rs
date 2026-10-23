// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! `AuthService`: the account flows' rules. Signing up, signing in (with
//! the lockout), email verification, the password flows, email change,
//! invitations and two-factor sign-in. Every write goes through this
//! feature's query module, `db::queries::auth`, or through a port; mail is
//! enqueued on the flow's own transaction and committed through the outbox.
//!
//! [`issue_token`] and [`consume_token`] are the single-use token lifecycle
//! every mailed link shares (verification, reset, email change,
//! invitation), and a pending two-factor sign-in's token too.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{TimeDelta, Utc};
use sqlx::{PgConnection, PgPool};

use super::dto::{
    ChangeEmailDto, ChangePasswordDto, CredentialsDto, DisableTotpDto, EnableTotpDto,
    ForgotPasswordDto, InviteUserDto, LoginResponse, ResetPasswordDto, TokenResponse,
    TotpEnabledResponse, TotpSetupResponse, VerifyTotpDto,
};
use crate::db::auth_tokens::models::AuthTokenId;
use crate::db::queries::auth::{
    self as queries, ConsumeTokenRow, InsertTokenParams, InviteUserParams,
};
use crate::db::totp_recovery_codes::models::TotpRecoveryCodeId;
use crate::db::types::{AuthTokenPurpose, Role};
use crate::db::users::models::{User, UserId};
use crate::db::users::read as users;
use crate::error::AppError;
use crate::shared::mail::{Mail, Outbox, Templates};
use crate::shared::security::breach::BreachCheck;
use crate::shared::security::jwt::JwtKeys;
use crate::shared::security::password;
use crate::shared::security::token::{self, SecretToken};
use crate::shared::security::totp::{self, TotpCipher};

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

/// How long a pending two-factor sign-in waits for its code, in minutes.
pub const TWO_FACTOR_PENDING_TTL_MINUTES: i64 = 5;

/// The `401` of every failed sign-in: it doesn't say which part was wrong,
/// nor whether the account exists.
const BAD_CREDENTIALS: &str = "invalid email or password, or the account is temporarily locked";

/// The `400` of a link that can't be used.
const INVALID_TOKEN: &str = "the link is invalid, expired or already used";

/// The `auth` service, built by `app.rs`.
#[derive(Clone)]
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
    /// Encrypts two-factor secrets at rest (`TOTP_ENCRYPTION_KEY`).
    pub totp: Arc<TotpCipher>,
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
    /// `require_verified_email`, is a `403`. Success forgets the failures,
    /// except on an account with two-factor on: it answers a pending token
    /// instead (valid [`TWO_FACTOR_PENDING_TTL_MINUTES`]), and the failures
    /// stay until [`AuthService::verify_totp`] succeeds.
    pub async fn login(&self, input: CredentialsDto) -> Result<LoginResponse, AppError> {
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
        if account.totp_enabled {
            let purpose = AuthTokenPurpose::TwoFactorPending;
            let mut tx = self.pool.begin().await?;
            let pending = issue_token(&mut tx, account.id, purpose, ttl_of(purpose), None).await?;
            tx.commit().await?;
            return Ok(LoginResponse::TotpRequired {
                requires_totp: true,
                pending_token: pending.raw,
            });
        }
        queries::clear_failed_logins(&self.pool, account.id).await?;
        let session = self.session(account.id, account.role, account.token_version)?;
        Ok(LoginResponse::Session(session))
    }

    /// A session for the account `id`, with `role` and `token_version`.
    fn session(&self, id: UserId, role: Role, version: i32) -> Result<TokenResponse, AppError> {
        Ok(TokenResponse {
            access_token: self.keys.issue(id.into(), role, version)?,
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
        let render = link_mail(purpose).ok_or_else(|| {
            AppError::Internal(format!("a `{}` token is never mailed", purpose.as_str()).into())
        })?;
        let link = issue_token(&mut *conn, user, purpose, ttl_of(purpose), new_email).await?;
        let mail = render(&self.templates, to, &link.raw);
        self.outbox.enqueue(conn, &mail).await
    }
}

/// Renders the message carrying a link: `(templates, to, raw token)`.
type RenderLink = fn(&Templates, &str, &str) -> Mail;

/// How long a `purpose` token lasts.
fn ttl_of(purpose: AuthTokenPurpose) -> TimeDelta {
    match purpose {
        AuthTokenPurpose::EmailVerification => TimeDelta::hours(VERIFICATION_TTL_HOURS),
        AuthTokenPurpose::PasswordReset => TimeDelta::hours(PASSWORD_RESET_TTL_HOURS),
        AuthTokenPurpose::EmailChange => TimeDelta::hours(EMAIL_CHANGE_TTL_HOURS),
        AuthTokenPurpose::Invitation => TimeDelta::days(INVITATION_TTL_DAYS),
        AuthTokenPurpose::TwoFactorPending => TimeDelta::minutes(TWO_FACTOR_PENDING_TTL_MINUTES),
    }
}

/// The message that carries a `purpose` link: none for a pending
/// two-factor sign-in, whose token sign-in answers instead.
fn link_mail(purpose: AuthTokenPurpose) -> Option<RenderLink> {
    match purpose {
        AuthTokenPurpose::EmailVerification => Some(Templates::verification),
        AuthTokenPurpose::PasswordReset => Some(Templates::password_reset),
        AuthTokenPurpose::EmailChange => Some(Templates::email_change),
        AuthTokenPurpose::Invitation => Some(Templates::invitation),
        AuthTokenPurpose::TwoFactorPending => None,
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
    /// token is spent. A reset recovers an account someone else may have
    /// had, so the same transaction revokes every API key it has and
    /// spends any pending two-factor sign-in.
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
        queries::revoke_all_api_keys(&mut *tx, spent.user_id).await?;
        queries::revoke_tokens(&mut *tx, spent.user_id, AuthTokenPurpose::TwoFactorPending).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Replaces the password of the account `id` when `current_password`
    /// is right (otherwise a `400 wrong_password`), signs every session
    /// out, the caller's included, and mails a notice. A breached password
    /// is a `400 breached_password`. The account's API keys keep working:
    /// whoever changes the password knew the old one, unlike a reset's.
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

/// Two-factor sign-in: setup, enable, disable, and a sign-in's second step.
impl AuthService {
    /// Starts a two-factor setup for the account `id`: a new secret, stored
    /// encrypted in place of any unconfirmed one, and what an authenticator
    /// app needs to enroll it. Nothing is enforced until
    /// [`AuthService::enable_totp`] confirms a code. Two-factor already on
    /// is a `400`.
    pub async fn setup_totp(&self, id: UserId) -> Result<TotpSetupResponse, AppError> {
        let user = self.live_user(id).await?;
        let enrollment = totp::enroll(&user.email)?;
        let sealed = self.totp.seal(&enrollment.secret)?;
        if queries::store_totp_secret(&self.pool, user.id, Some(&sealed)).await? == 0 {
            return Err(totp_already_on());
        }
        Ok(TotpSetupResponse {
            secret: enrollment.base32,
            qr_code_data_uri: enrollment.qr_code_data_uri,
            provisioning_uri: enrollment.provisioning_uri,
        })
    }

    /// Turns two-factor on for the account `id` with a code of its setup's
    /// secret: ten recovery codes, answered once and stored hashed, and a
    /// notice mailed, on one transaction. A wrong code, no setup, or
    /// two-factor already on is a `400`.
    pub async fn enable_totp(
        &self,
        id: UserId,
        input: EnableTotpDto,
    ) -> Result<TotpEnabledResponse, AppError> {
        let user = self.live_user(id).await?;
        if user.totp_enabled {
            return Err(totp_already_on());
        }
        let sealed = user
            .totp_secret
            .ok_or_else(|| AppError::BadRequest("no two-factor setup is in progress".to_owned()))?;
        let step = (self.totp)
            .check(&sealed, input.code.trim(), user.totp_last_step)?
            .ok_or_else(|| AppError::BadRequest("the code is wrong or expired".to_owned()))?;
        let codes = totp::recovery_codes();
        let mut tx = self.pool.begin().await?;
        if queries::enable_totp(&mut *tx, user.id, step, &sealed).await? == 0 {
            return Err(AppError::BadRequest(
                "two-factor was enabled or set up again meanwhile".to_owned(),
            ));
        }
        queries::delete_recovery_codes(&mut *tx, user.id).await?;
        for code in &codes {
            let code_id = TotpRecoveryCodeId::generate();
            queries::insert_recovery_code(&mut *tx, code_id, user.id, &code.hash).await?;
        }
        let notice = self.templates.two_factor_enabled(&user.email);
        self.outbox.enqueue(&mut tx, &notice).await?;
        self.outbox.commit(tx).await?;
        Ok(TotpEnabledResponse {
            recovery_codes: codes.into_iter().map(|code| code.raw).collect(),
        })
    }

    /// Turns two-factor off for the account `id` when `current_password` is
    /// right (otherwise a `400 wrong_password`): its secret and recovery
    /// codes are forgotten and, when it was on, a notice is mailed.
    pub async fn disable_totp(&self, id: UserId, input: DisableTotpDto) -> Result<(), AppError> {
        let user = self.live_user(id).await?;
        if !password::verify(input.current_password, user.password_hash).await? {
            return Err(wrong_password());
        }
        let mut tx = self.pool.begin().await?;
        queries::disable_totp(&mut *tx, user.id).await?;
        queries::delete_recovery_codes(&mut *tx, user.id).await?;
        if user.totp_enabled {
            let notice = self.templates.two_factor_disabled(&user.email);
            self.outbox.enqueue(&mut tx, &notice).await?;
        }
        self.outbox.commit(tx).await
    }

    /// A two-factor sign-in's second step: the pending token
    /// [`AuthService::login`] answered, with a code of the account's
    /// authenticator or one of its unused recovery codes, for a session. A
    /// bad, expired or spent token is a `400 invalid_token`. A wrong code,
    /// or a locked account, is a `401`; a wrong code counts toward the
    /// lockout like a wrong password, and the token stays usable until it
    /// expires. Success spends the token and the code, and forgets the
    /// failures. A disabled account is a `403`.
    pub async fn verify_totp(&self, input: VerifyTotpDto) -> Result<TokenResponse, AppError> {
        let pending = token::hash(&input.pending_token);
        let id = queries::pending_sign_in(&self.pool, &pending)
            .await?
            .ok_or_else(invalid_token)?;
        let user = users::get_by_id(&self.pool, id)
            .await?
            .ok_or_else(invalid_token)?;
        let wrong_code = || {
            AppError::Unauthorized("the code is wrong or used, or the account is locked".to_owned())
        };
        if user.locked_until.is_some_and(|until| until > Utc::now()) {
            return Err(wrong_code());
        }
        if !user.is_active {
            return Err(AppError::Forbidden("the account is disabled".to_owned()));
        }
        let mut tx = self.pool.begin().await?;
        if !self.spend_code(&mut tx, &user, input.code.trim()).await? {
            tx.rollback().await?;
            queries::record_failed_login(&self.pool, user.id, MAX_FAILED_LOGINS, LOCK_MINUTES)
                .await?;
            return Err(wrong_code());
        }
        let purpose = AuthTokenPurpose::TwoFactorPending;
        consume_token(&mut tx, &input.pending_token, purpose).await?;
        queries::clear_failed_logins(&mut *tx, user.id).await?;
        tx.commit().await?;
        self.session(user.id, user.role, user.token_version)
    }

    /// Spends `code` on `conn` when it is a right code of `user`'s
    /// authenticator of a step not used yet, or one of its unused recovery
    /// codes; `false` (nothing spent) otherwise, or when two-factor is off.
    async fn spend_code(
        &self,
        conn: &mut PgConnection,
        user: &User,
        code: &str,
    ) -> Result<bool, AppError> {
        let (true, Some(sealed)) = (user.totp_enabled, user.totp_secret.as_deref()) else {
            return Ok(false);
        };
        if !totp::is_code(code) {
            let hash = totp::recovery_hash(code);
            return Ok(queries::use_recovery_code(&mut *conn, user.id, &hash).await? == 1);
        }
        match self.totp.check(sealed, code, user.totp_last_step)? {
            Some(step) => Ok(queries::accept_totp_step(&mut *conn, user.id, step).await? == 1),
            None => Ok(false),
        }
    }

    /// The live account `id`, or a `401`: a session for an account that
    /// no longer exists.
    async fn live_user(&self, id: UserId) -> Result<User, AppError> {
        users::get_by_id(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::Unauthorized("the account no longer exists".to_owned()))
    }
}

/// The `400` of a setup or enable when two-factor is already on.
fn totp_already_on() -> AppError {
    AppError::BadRequest("two-factor sign-in is already on".to_owned())
}
