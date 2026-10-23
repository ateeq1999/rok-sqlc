// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! `TwoFactorService`: two-factor sign-in's rules. Setting it up, turning
//! it on and off, and a sign-in's second step. `auth`'s sign-in answers a
//! pending token for an account with two-factor on
//! (`shared::security::auth_token`); [`TwoFactorService::verify_totp`]
//! redeems it. The cryptography is `shared::security::totp`; every write
//! goes through this feature's query module, `db::queries::two_factor`.

use std::sync::Arc;

use chrono::Utc;
use sqlx::{PgConnection, PgPool};

use super::dto::{
    DisableTotpDto, EnableTotpDto, TokenResponse, TotpEnabledResponse, TotpSetupResponse,
    VerifyTotpDto,
};
use crate::db::queries::two_factor as queries;
use crate::db::totp_recovery_codes::models::TotpRecoveryCodeId;
use crate::db::types::{AuthTokenPurpose, Role};
use crate::db::users::models::{User, UserId};
use crate::db::users::read as users;
use crate::error::AppError;
use crate::shared::mail::{Outbox, Templates};
use crate::shared::security::auth_token::{consume_token, invalid_token};
use crate::shared::security::jwt::JwtKeys;
use crate::shared::security::password::{self, LOCK_MINUTES, MAX_FAILED_LOGINS};
use crate::shared::security::token;
use crate::shared::security::totp::{self, TotpCipher};

/// The `two_factor` service, built by `app.rs`.
#[derive(Clone)]
pub struct TwoFactorService {
    /// The pool the flows read and write through.
    pub pool: PgPool,
    /// Signs the session a completed sign-in answers.
    pub keys: Arc<JwtKeys>,
    /// Where the notices go, on the flow's transaction.
    pub outbox: Arc<Outbox>,
    /// The notices the flows send.
    pub templates: Arc<Templates>,
    /// Encrypts two-factor secrets at rest (`TOTP_ENCRYPTION_KEY`).
    pub totp: Arc<TotpCipher>,
}

/// Setup, enable, disable, and a sign-in's second step.
impl TwoFactorService {
    /// Starts a two-factor setup for the account `id`: a new secret, stored
    /// encrypted in place of any unconfirmed one, and what an authenticator
    /// app needs to enroll it. Nothing is enforced until
    /// [`TwoFactorService::enable_totp`] confirms a code. Two-factor already on
    /// is a `400`.
    pub async fn setup_totp(&self, id: UserId) -> Result<TotpSetupResponse, AppError> {
        let user = self.live_user(id).await?;
        let enrollment = totp::enroll(&user.email)?;
        let sealed = self.totp.seal(user.id.into(), &enrollment.secret)?;
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
            .check(
                user.id.into(),
                &sealed,
                input.code.trim(),
                user.totp_last_step,
            )?
            .ok_or_else(|| AppError::BadRequest("the code is wrong or expired".to_owned()))?;
        let codes = self.totp.recovery_codes();
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
    /// codes are forgotten, every session is signed out (the caller's
    /// included, as a password change does) and, when it was on, a notice
    /// is mailed. The contract asks only for the password, not a code.
    pub async fn disable_totp(&self, id: UserId, input: DisableTotpDto) -> Result<(), AppError> {
        let user = self.live_user(id).await?;
        if !password::verify(input.current_password, user.password_hash).await? {
            return Err(AppError::WrongPassword(
                "the current password is wrong".to_owned(),
            ));
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
    /// `AuthService::login` answered, with a code of the account's
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
            queries::record_failed_code(&self.pool, user.id, MAX_FAILED_LOGINS, LOCK_MINUTES)
                .await?;
            return Err(wrong_code());
        }
        let purpose = AuthTokenPurpose::TwoFactorPending;
        consume_token(&mut tx, &input.pending_token, purpose).await?;
        queries::clear_failed_codes(&mut *tx, user.id).await?;
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
            let hash = self.totp.recovery_hash(code);
            return Ok(queries::use_recovery_code(&mut *conn, user.id, &hash).await? == 1);
        }
        match self
            .totp
            .check(user.id.into(), sealed, code, user.totp_last_step)?
        {
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

impl TwoFactorService {
    /// A session for `id`, as `auth`'s sign-in answers one.
    fn session(&self, id: UserId, role: Role, version: i32) -> Result<TokenResponse, AppError> {
        Ok(TokenResponse {
            access_token: self.keys.issue(id.into(), role, version)?,
            expires_in: i64::try_from(self.keys.ttl().as_secs()).unwrap_or(i64::MAX),
            token_type: "Bearer".to_owned(),
        })
    }
}
