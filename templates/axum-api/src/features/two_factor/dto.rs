// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The `two_factor` request and response bodies, named as the contract
//! names them. `TokenResponse` and `MessageResponse` are the same schemas
//! as `auth`'s: a feature keeps its own copies rather than reach into
//! another's.

/// A session: a bearer token and how long it lasts.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct TokenResponse {
    /// The JWT to send as `Authorization: Bearer <access_token>`.
    pub access_token: String,
    /// Seconds until it expires.
    pub expires_in: i64,
    /// Always `Bearer`.
    pub token_type: String,
}

/// A message for people, such as `email verified`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct MessageResponse {
    /// What happened.
    pub message: String,
}

impl MessageResponse {
    /// The response saying `message`.
    #[must_use]
    pub fn new(message: &str) -> Self {
        MessageResponse {
            message: message.to_owned(),
        }
    }
}

/// Everything needed to enroll an authenticator app. Two-factor is not yet
/// on: the caller confirms a code from the app with `POST /auth/2fa/enable`
/// first.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct TotpSetupResponse {
    /// Base32-encoded; lets the user type the secret in by hand if they
    /// cannot scan the QR code.
    pub secret: String,
    /// The QR code of `provisioning_uri`, as a `data:image/svg+xml` URI.
    pub qr_code_data_uri: String,
    /// The `otpauth://totp/…` URI the QR code carries.
    pub provisioning_uri: String,
}

/// Shown exactly once, right after enabling two-factor: only hashes of
/// these are ever stored.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct TotpEnabledResponse {
    /// Ten single-use `XXXXX-XXXXX` codes, each standing in for an
    /// authenticator code once.
    pub recovery_codes: Vec<String>,
}

/// The longest code `POST /auth/2fa/enable` and `/verify` take.
pub const CODE_MAX: u64 = 32;

/// `POST /auth/2fa/enable`'s body: a code from the authenticator app.
#[derive(Debug, Clone, serde::Deserialize, validator::Validate, utoipa::ToSchema)]
pub struct EnableTotpDto {
    /// The app's current 6-digit code.
    #[validate(length(min = 1, max = CODE_MAX, message = "code must be 1 to 32 characters"))]
    pub code: String,
}

/// `POST /auth/2fa/disable`'s body. Requires the current password so a
/// stolen session token alone cannot turn off two-factor.
#[derive(Debug, Clone, serde::Deserialize, validator::Validate, utoipa::ToSchema)]
pub struct DisableTotpDto {
    /// The account's password now.
    pub current_password: String,
}

/// `POST /auth/2fa/verify`'s body. `code` accepts either a live 6-digit
/// authenticator code or an `XXXXX-XXXXX` recovery code.
#[derive(Debug, Clone, serde::Deserialize, validator::Validate, utoipa::ToSchema)]
pub struct VerifyTotpDto {
    /// The token `POST /auth/login` answered with `requires_totp`.
    #[validate(length(min = 1, message = "pending_token must not be empty"))]
    pub pending_token: String,
    /// An authenticator code or a recovery code.
    #[validate(length(min = 1, max = CODE_MAX, message = "code must be 1 to 32 characters"))]
    pub code: String,
}
