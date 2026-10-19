// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Whether a new password has appeared in a known data breach: the
//! [`BreachCheck`] port, checked by sign-up, password reset and password
//! change before hashing. [`PwnedPasswords`] asks Have I Been Pwned's range
//! API; [`NoBreachCheck`] (the default, `BREACH_CHECK=false`) asks nobody.
//!
//! The check fails open: when the service can't be reached or answers
//! nonsense, the password is accepted and a warning is logged, so an outage
//! elsewhere never blocks a sign-up.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use sha1::{Digest, Sha1};

use crate::config::Config;

/// Have I Been Pwned's range API; a request appends the hash's first five
/// hex characters.
pub const PWNED_PASSWORDS_URL: &str = "https://api.pwnedpasswords.com/range/";

/// How long [`PwnedPasswords`] waits for an answer before giving up (and
/// accepting the password).
const TIMEOUT: Duration = Duration::from_secs(3);

/// The port: does `password` appear in a known breach?
#[async_trait]
pub trait BreachCheck: Send + Sync {
    /// `true` only when the password is known to be breached; `false` when
    /// it isn't, or when that can't be found out.
    async fn is_breached(&self, password: &str) -> bool;
}

/// The check that never finds anything: `BREACH_CHECK=false`.
pub struct NoBreachCheck;

#[async_trait]
impl BreachCheck for NoBreachCheck {
    async fn is_breached(&self, _: &str) -> bool {
        false
    }
}

/// Have I Been Pwned's k-anonymity range API: only the first five hex
/// characters of the password's SHA-1 leave the process, and the answer
/// lists every breached hash sharing them (padded, so its size says
/// nothing either).
pub struct PwnedPasswords {
    client: reqwest::Client,
    base_url: String,
}

impl PwnedPasswords {
    /// The check against `base_url` ([`PWNED_PASSWORDS_URL`] in production).
    #[must_use]
    pub fn new(base_url: &str) -> Self {
        PwnedPasswords {
            client: reqwest::Client::new(),
            base_url: base_url.to_owned(),
        }
    }

    /// The range's body, or why it couldn't be read.
    async fn range(&self, prefix: &str) -> Result<String, reqwest::Error> {
        self.client
            .get(format!("{}{prefix}", self.base_url))
            .header("Add-Padding", "true")
            .timeout(TIMEOUT)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await
    }
}

#[async_trait]
impl BreachCheck for PwnedPasswords {
    async fn is_breached(&self, password: &str) -> bool {
        let (prefix, suffix) = split_hash(password);
        match self.range(&prefix).await {
            Ok(body) => listed(&body, &suffix),
            Err(err) => {
                tracing::warn!(error = %err, "the breached-password check failed; accepting the password");
                false
            }
        }
    }
}

/// The check `config` asks for: [`PwnedPasswords`] when `BREACH_CHECK` is
/// on, else [`NoBreachCheck`].
#[must_use]
pub fn from_config(config: &Config) -> Arc<dyn BreachCheck> {
    if config.breach_check {
        Arc::new(PwnedPasswords::new(PWNED_PASSWORDS_URL))
    } else {
        Arc::new(NoBreachCheck)
    }
}

/// The password's SHA-1 in uppercase hex, split into the five characters
/// sent and the 35 compared locally.
fn split_hash(password: &str) -> (String, String) {
    let hex: String = Sha1::digest(password.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect();
    let (prefix, suffix) = hex.split_at(5);
    (prefix.to_owned(), suffix.to_owned())
}

/// Whether a range body (`SUFFIX:COUNT` lines) lists `suffix` with a count
/// above zero. Padding lines have a count of zero.
fn listed(body: &str, suffix: &str) -> bool {
    body.lines().any(|line| {
        line.trim()
            .split_once(':')
            .is_some_and(|(candidate, count)| {
                candidate.eq_ignore_ascii_case(suffix)
                    && count.trim().parse::<u64>().is_ok_and(|n| n > 0)
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_is_split_into_the_five_characters_sent_and_the_rest() {
        // SHA-1("password") = 5BAA61E4C9B93F3F0682250B6CF8331B7EE68FD8
        let (prefix, suffix) = split_hash("password");
        assert_eq!(prefix, "5BAA6");
        assert_eq!(suffix, "1E4C9B93F3F0682250B6CF8331B7EE68FD8");
    }

    #[test]
    fn a_suffix_is_listed_only_with_a_count_above_zero() {
        let body = "0018A45C4D1DEF81644B54AB7F969B88D65:1\r\n\
                    1E4C9B93F3F0682250B6CF8331B7EE68FD8:9659365\r\n\
                    1E4C9B93F3F0682250B6CF8331B7EE68FD9:0\r\n";
        assert!(listed(body, "1E4C9B93F3F0682250B6CF8331B7EE68FD8"));
        assert!(listed(body, "1e4c9b93f3f0682250b6cf8331b7ee68fd8"));
        assert!(!listed(body, "1E4C9B93F3F0682250B6CF8331B7EE68FD9"));
        assert!(!listed(body, "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF"));
    }

    /// Nothing listens on the discard port, so the request fails at once:
    /// the check fails open.
    #[actix_web::test]
    async fn an_unreachable_service_accepts_the_password() {
        let check = PwnedPasswords::new("http://127.0.0.1:9/range/");
        assert!(!check.is_breached("password").await);
    }
}
