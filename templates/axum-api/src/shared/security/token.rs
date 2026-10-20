// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Single-use tokens mailed as links (email verification, password reset,
//! email change, invitation): 32 random bytes, base64url. Only the SHA-256
//! of a token is stored, so a leaked `auth_tokens` table holds no usable
//! link. The raw token does sit in the mail outbox (`outbound_mail`) while
//! its mail is pending, since that row is what gets sent; a sent row is
//! deleted, and a `dead` one loses its bodies, so no link outlives its
//! delivery.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

/// A new token: what the link carries and what the database keeps.
pub struct SecretToken {
    /// Sent to the user, inside a link; never stored.
    pub raw: String,
    /// What `auth_tokens.token_hash` keeps: [`hash`] of `raw`.
    pub hash: String,
}

impl SecretToken {
    /// 32 random bytes, base64url without padding (43 characters).
    #[must_use]
    pub fn generate() -> Self {
        let bytes: [u8; 32] = rand::random();
        let raw = URL_SAFE_NO_PAD.encode(bytes);
        let hash = hash(&raw);
        SecretToken { raw, hash }
    }
}

/// The SHA-256 of `raw`, in lowercase hex: how a presented token is looked
/// up. A token is random, so a fast hash is enough (unlike a password).
#[must_use]
pub fn hash(raw: &str) -> String {
    Sha256::digest(raw.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_43_base64url_characters_stored_as_its_sha256_hex() {
        let token = SecretToken::generate();
        assert_eq!(token.raw.len(), 43);
        assert!(
            token
                .raw
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "{}",
            token.raw
        );
        assert_eq!(token.hash, hash(&token.raw));
        assert_ne!(SecretToken::generate().raw, token.raw);
    }

    #[test]
    fn the_hash_is_sha256_in_lowercase_hex() {
        assert_eq!(
            hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
