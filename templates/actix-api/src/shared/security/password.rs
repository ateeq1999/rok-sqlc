// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Passwords: the policy (8 to 128 characters, no composition rules, as
//! NIST 800-63B advises) and argon2id hashing with the crate's recommended
//! parameters. Hashing is slow on purpose, so [`hash`] and [`verify`] run
//! on the blocking thread pool, never on the server's workers.

use actix_web::web;
use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};

use crate::error::AppError;

/// The shortest password the policy accepts, in characters.
pub const MIN_LENGTH: u64 = 8;

/// The longest password the policy accepts, in characters. It bounds the
/// work a hash does.
pub const MAX_LENGTH: u64 = 128;

/// An argon2id hash (of a random password nobody knows) with the
/// parameters [`hash`] uses. [`verify`] checks a sign-in for an unknown
/// email against it, so the answer takes as long as for a known one and
/// timing doesn't reveal which emails have accounts.
pub const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$T0V7mpBKr+vrdERBVNGeyw$Hbq9haKkL3j/od7wLk2OSuN9q0pzh65S5apfwNXsjf4";

/// Whether `password` meets the policy: [`MIN_LENGTH`] to [`MAX_LENGTH`]
/// characters, anything else allowed. A body's `#[validate(length(...))]`
/// states the same bounds.
#[must_use]
pub fn within_policy(password: &str) -> bool {
    let length = password.chars().count() as u64;
    (MIN_LENGTH..=MAX_LENGTH).contains(&length)
}

/// The argon2id hash of `password`, as a PHC string for
/// `users.password_hash`.
pub async fn hash(password: String) -> Result<String, AppError> {
    web::block(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|hashed| hashed.to_string())
            .map_err(|err| AppError::Internal(format!("password hashing failed: {err}").into()))
    })
    .await
    .map_err(|err| AppError::Internal(Box::new(err)))?
}

/// Whether `password` matches `stored`, the account's hash. With no stored
/// hash (an unknown email, or an invited account with no password yet) it
/// checks against [`DUMMY_HASH`], which never matches, so the call costs
/// the same either way.
pub async fn verify(password: String, stored: Option<String>) -> Result<bool, AppError> {
    web::block(move || {
        let known = stored.is_some();
        let stored = stored.unwrap_or_else(|| DUMMY_HASH.to_owned());
        let matches = PasswordHash::new(&stored).is_ok_and(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        });
        known && matches
    })
    .await
    .map_err(|err| AppError::Internal(Box::new(err)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[actix_web::test]
    async fn a_hash_verifies_its_password_and_no_other() -> Result<(), AppError> {
        let hashed = hash("correct horse".to_owned()).await?;
        assert!(hashed.starts_with("$argon2id$"), "{hashed}");
        assert!(verify("correct horse".to_owned(), Some(hashed.clone())).await?);
        assert!(!verify("wrong horse".to_owned(), Some(hashed)).await?);
        Ok(())
    }

    #[actix_web::test]
    async fn an_unknown_account_is_verified_against_the_dummy_hash_and_fails()
    -> Result<(), AppError> {
        assert!(!verify("correct horse".to_owned(), None).await?);
        Ok(())
    }

    /// The dummy hash costs what a real one does: same algorithm and
    /// parameters as [`hash`] writes.
    #[actix_web::test]
    async fn the_dummy_hash_has_the_parameters_of_a_real_one() -> Result<(), AppError> {
        let real = hash("anything".to_owned()).await?;
        let params = |phc: &str| phc.rsplitn(3, '$').nth(2).map(str::to_owned);
        assert_eq!(params(DUMMY_HASH), params(&real));
        Ok(())
    }

    #[test]
    fn the_policy_is_8_to_128_characters() {
        assert!(!within_policy("1234567"));
        assert!(within_policy("12345678"));
        assert!(within_policy(&"x".repeat(128)));
        assert!(!within_policy(&"x".repeat(129)));
        // Characters, not bytes: eight two-byte characters are enough.
        assert!(within_policy("éééééééé"));
        assert!(!within_policy("ééé"));
    }
}
