// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Two-factor sign-in's cryptography, with no business meaning: the
//! authenticator secret and its enrollment (the provisioning URI and its QR
//! code), the codes (TOTP, RFC 6238: SHA-1, 6 digits, 30-second steps, one
//! step of drift either way), the secret's encryption at rest (AES-256-GCM
//! under `TOTP_ENCRYPTION_KEY`), and the recovery codes.
//!
//! A code is matched to its time step, so the caller can store the last
//! step it accepted and refuse that step (and older ones) next time: a code
//! is never accepted twice.

use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use qrcode::QrCode;
use qrcode::render::svg;
use totp_rs::{Algorithm, TOTP};

use super::token;
use crate::error::AppError;

/// The digits of a code.
pub const DIGITS: usize = 6;

/// How long a code lasts, in seconds.
pub const STEP_SECS: u64 = 30;

/// How many steps a code may be off by, either way, for clock drift.
pub const DRIFT_STEPS: i64 = 1;

/// The bytes of a new secret: 160 bits, as RFC 4226 recommends.
pub const SECRET_BYTES: usize = 20;

/// How many recovery codes enabling two-factor issues.
pub const RECOVERY_CODES: usize = 10;

/// The bytes of `TOTP_ENCRYPTION_KEY`: an AES-256 key.
pub const KEY_BYTES: usize = 32;

/// The bytes of an AES-GCM nonce.
const NONCE_BYTES: usize = 12;

/// The name authenticator apps show beside the account: the crate's.
const ISSUER: &str = env!("CARGO_PKG_NAME");

/// The characters of a recovery code: no `0`/`O` or `1`/`I`, which read
/// alike.
const RECOVERY_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// The characters on each side of a recovery code's dash.
const RECOVERY_HALF: usize = 5;

/// Encrypts and decrypts stored secrets with `TOTP_ENCRYPTION_KEY`. It has
/// no `Debug`, so the key can't end up in a log.
#[derive(Clone)]
pub struct TotpCipher(Aes256Gcm);

impl TotpCipher {
    /// The cipher under `key`.
    #[must_use]
    pub fn new(key: &[u8; KEY_BYTES]) -> Self {
        TotpCipher(Aes256Gcm::new(key.into()))
    }

    /// `secret`, encrypted under a fresh random nonce: base64 of the nonce
    /// followed by the ciphertext (with its tag), as `users.totp_secret`
    /// keeps it.
    pub fn seal(&self, secret: &[u8]) -> Result<String, AppError> {
        let nonce: [u8; NONCE_BYTES] = rand::random();
        let mut sealed = nonce.to_vec();
        sealed.extend(
            self.0
                .encrypt(Nonce::from_slice(&nonce), secret)
                .map_err(|_| AppError::Internal("encrypting a TOTP secret failed".into()))?,
        );
        Ok(STANDARD.encode(sealed))
    }

    /// The secret [`TotpCipher::seal`] encrypted as `sealed`. A value that
    /// isn't one, or was sealed under another key, is an internal error.
    pub fn open(&self, sealed: &str) -> Result<Vec<u8>, AppError> {
        let unreadable = || AppError::Internal("a stored TOTP secret can't be decrypted".into());
        let bytes = STANDARD.decode(sealed).map_err(|_| unreadable())?;
        if bytes.len() <= NONCE_BYTES {
            return Err(unreadable());
        }
        let (nonce, ciphertext) = bytes.split_at(NONCE_BYTES);
        self.0
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| unreadable())
    }

    /// The step `code` is the code of now (see [`matching_step`]) for the
    /// secret sealed as `sealed`, after `last`; `None`: it is no such code.
    pub fn check(
        &self,
        sealed: &str,
        code: &str,
        last: Option<i64>,
    ) -> Result<Option<i64>, AppError> {
        let secret = self.open(sealed)?;
        Ok(matching_step(&secret, code, current_step(), last))
    }
}

/// A new secret and what an authenticator app needs to enroll it.
pub struct Enrollment {
    /// The secret's bytes: seal them with [`TotpCipher::seal`] to store.
    pub secret: Vec<u8>,
    /// The secret in base32, for typing in by hand.
    pub base32: String,
    /// The `otpauth://totp/…` URI the QR code carries.
    pub provisioning_uri: String,
    /// The QR code of `provisioning_uri`: `data:image/svg+xml;base64,…`,
    /// ready for an `<img src>`.
    pub qr_code_data_uri: String,
}

/// A new random secret for `account` (the email the app shows), with its
/// provisioning URI and QR code.
pub fn enroll(account: &str) -> Result<Enrollment, AppError> {
    let secret: [u8; SECRET_BYTES] = rand::random();
    let totp = authenticator(secret.to_vec(), account);
    let provisioning_uri = totp.get_url();
    let qr = QrCode::new(provisioning_uri.as_bytes())
        .map_err(|err| AppError::Internal(format!("drawing the QR code failed: {err}").into()))?
        .render::<svg::Color>()
        .min_dimensions(200, 200)
        .build();
    Ok(Enrollment {
        base32: totp.get_secret_base32(),
        secret: secret.to_vec(),
        provisioning_uri,
        qr_code_data_uri: format!("data:image/svg+xml;base64,{}", STANDARD.encode(qr)),
    })
}

/// The time step of now.
#[must_use]
pub fn current_step() -> i64 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    i64::try_from(secs / STEP_SECS).unwrap_or(i64::MAX)
}

/// The step `code` is the code of for `secret`, among `now` and the
/// [`DRIFT_STEPS`] on either side of it, skipping `last` (the last step
/// accepted) and every step before it. `None`: no step matches.
#[must_use]
pub fn matching_step(secret: &[u8], code: &str, now: i64, last: Option<i64>) -> Option<i64> {
    if !is_code(code) {
        return None;
    }
    let totp = authenticator(secret.to_vec(), "");
    (now - DRIFT_STEPS..=now + DRIFT_STEPS)
        .filter(|step| *step >= 0 && last.is_none_or(|last| *step > last))
        .find(|step| {
            let time = u64::try_from(*step).unwrap_or(0) * STEP_SECS;
            constant_time_eq(totp.generate(time).as_bytes(), code.as_bytes())
        })
}

/// Whether `code` has a code's shape: [`DIGITS`] ASCII digits. Anything
/// else a sign-in sends is taken for a recovery code.
#[must_use]
pub fn is_code(code: &str) -> bool {
    code.len() == DIGITS && code.bytes().all(|byte| byte.is_ascii_digit())
}

/// A new recovery code: what is shown once and what is stored.
pub struct RecoveryCode {
    /// `XXXXX-XXXXX`, shown to the user once; never stored.
    pub raw: String,
    /// What `totp_recovery_codes.code_hash` keeps: [`recovery_hash`] of
    /// `raw`.
    pub hash: String,
}

/// [`RECOVERY_CODES`] new recovery codes, each `XXXXX-XXXXX` from a
/// 32-character alphabet (50 random bits).
#[must_use]
pub fn recovery_codes() -> Vec<RecoveryCode> {
    (0..RECOVERY_CODES)
        .map(|_| {
            let bytes: [u8; 2 * RECOVERY_HALF] = rand::random();
            let mut raw: String = bytes
                .iter()
                .map(|byte| char::from(RECOVERY_ALPHABET[usize::from(*byte) % 32]))
                .collect();
            raw.insert(RECOVERY_HALF, '-');
            let hash = recovery_hash(&raw);
            RecoveryCode { raw, hash }
        })
        .collect()
}

/// The SHA-256 (lowercase hex) a presented recovery code is looked up by:
/// trimmed and uppercased first, so `abcde-fghjk` finds `ABCDE-FGHJK`.
#[must_use]
pub fn recovery_hash(code: &str) -> String {
    token::hash(&code.trim().to_uppercase())
}

/// The authenticator of `secret` for `account`, under [`ISSUER`].
fn authenticator(secret: Vec<u8>, account: &str) -> TOTP {
    TOTP::new_unchecked(
        Algorithm::SHA1,
        DIGITS,
        0,
        STEP_SECS,
        secret,
        Some(ISSUER.to_owned()),
        account.to_owned(),
    )
}

/// Whether `a` and `b` are equal, in a time that doesn't depend on where
/// they differ.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |diff, (x, y)| diff | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use totp_rs::Secret;

    use super::*;

    /// RFC 6238's SHA-1 secret: the ASCII of `12345678901234567890`.
    const RFC_SECRET: &[u8] = b"12345678901234567890";

    #[test]
    fn codes_match_rfc_6238s_test_vectors() {
        // RFC 6238, appendix B: 94287082 at 59 s (step 1) and 07081804 at
        // 1111111109 s (step 37037036); the low 6 digits are the code.
        assert_eq!(matching_step(RFC_SECRET, "287082", 1, None), Some(1));
        assert_eq!(
            matching_step(RFC_SECRET, "081804", 37_037_036, None),
            Some(37_037_036)
        );
        assert_eq!(matching_step(RFC_SECRET, "287083", 1, None), None);
    }

    #[test]
    fn a_code_is_accepted_one_step_either_way_and_no_further() {
        assert_eq!(
            matching_step(RFC_SECRET, "081804", 37_037_035, None),
            Some(37_037_036)
        );
        assert_eq!(
            matching_step(RFC_SECRET, "081804", 37_037_037, None),
            Some(37_037_036)
        );
        assert_eq!(matching_step(RFC_SECRET, "081804", 37_037_034, None), None);
        assert_eq!(matching_step(RFC_SECRET, "081804", 37_037_038, None), None);
    }

    #[test]
    fn a_step_at_or_before_the_last_accepted_is_refused() {
        let step = 37_037_036;
        assert_eq!(matching_step(RFC_SECRET, "081804", step, Some(step)), None);
        assert_eq!(
            matching_step(RFC_SECRET, "081804", step, Some(step + 1)),
            None
        );
        assert_eq!(
            matching_step(RFC_SECRET, "081804", step, Some(step - 1)),
            Some(step)
        );
    }

    #[test]
    fn only_six_digits_are_a_code() {
        assert!(is_code("012345"));
        assert!(!is_code("12345"));
        assert!(!is_code("1234567"));
        assert!(!is_code("12345a"));
        assert!(!is_code("ABCDE-FGHJK"));
    }

    #[test]
    fn a_sealed_secret_opens_only_under_its_key() -> Result<(), AppError> {
        let cipher = TotpCipher::new(&[7; KEY_BYTES]);
        let sealed = cipher.seal(RFC_SECRET)?;
        assert!(!sealed.contains("12345678901234567890"));
        assert_eq!(
            STANDARD.decode(&sealed).map(|b| b.len()).ok(),
            Some(12 + 20 + 16)
        );
        assert_ne!(cipher.seal(RFC_SECRET)?, sealed, "a fresh nonce each time");
        assert_eq!(cipher.open(&sealed)?, RFC_SECRET);
        assert!(TotpCipher::new(&[8; KEY_BYTES]).open(&sealed).is_err());
        assert!(cipher.open("not base64!").is_err());
        assert!(cipher.open("c2hvcnQ=").is_err());

        let enrollment = enroll("ada@example.com")?;
        let sealed = cipher.seal(&enrollment.secret)?;
        let now = current_step();
        let code = authenticator(enrollment.secret, "")
            .generate(u64::try_from(now).unwrap_or(0) * STEP_SECS);
        let step = cipher.check(&sealed, &code, None)?;
        assert_eq!(step, Some(now));
        assert_eq!(cipher.check(&sealed, &code, step)?, None);
        Ok(())
    }

    #[test]
    fn an_enrollment_carries_the_secret_in_base32_its_uri_and_a_qr_code() -> Result<(), AppError> {
        let enrollment = enroll("ada@example.com")?;
        assert_eq!(enrollment.secret.len(), SECRET_BYTES);
        assert_eq!(enrollment.base32.len(), 32);
        assert_eq!(
            Secret::Encoded(enrollment.base32.clone()).to_bytes().ok(),
            Some(enrollment.secret.clone())
        );
        assert_eq!(
            enrollment.provisioning_uri,
            format!(
                "otpauth://totp/{ISSUER}:ada%40example.com?secret={}&issuer={ISSUER}",
                enrollment.base32
            )
        );
        let svg = enrollment
            .qr_code_data_uri
            .strip_prefix("data:image/svg+xml;base64,")
            .and_then(|data| STANDARD.decode(data).ok())
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_default();
        assert!(svg.contains("<svg"), "{svg}");
        assert_ne!(enroll("ada@example.com")?.secret, enrollment.secret);
        Ok(())
    }

    #[test]
    fn recovery_codes_are_ten_distinct_shown_once_codes_stored_as_hashes() {
        let codes = recovery_codes();
        assert_eq!(codes.len(), 10);
        for code in &codes {
            assert_eq!(code.raw.len(), 11, "{}", code.raw);
            assert_eq!(code.raw.as_bytes()[5], b'-');
            assert!(
                code.raw
                    .bytes()
                    .all(|byte| byte == b'-' || RECOVERY_ALPHABET.contains(&byte)),
                "{}",
                code.raw
            );
            assert_eq!(code.hash, token::hash(&code.raw));
            assert_eq!(
                recovery_hash(&format!(" {} ", code.raw.to_lowercase())),
                code.hash
            );
        }
        let distinct: std::collections::BTreeSet<_> = codes.iter().map(|code| &code.raw).collect();
        assert_eq!(distinct.len(), 10);
    }
}
