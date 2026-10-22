// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The application's settings, read from the environment into one
//! [`Config`]. This is the only file that reads environment variables.
//! `.env.example` lists them all.
//!
//! | Variable | Default | Setting |
//! |---|---|---|
//! | `DATABASE_URL` | required | the PostgreSQL URL |
//! | `BIND_ADDR` | `0.0.0.0:8080` | where the server listens |
//! | `JWT_SECRET` | required | the HS256 key tokens are signed with, at least 32 bytes |
//! | `JWT_TTL` | 3600 | seconds an issued token lasts |
//! | `REQUIRE_VERIFIED_EMAIL` | `true` | sign-in needs a verified email |
//! | `APP_URL` | `http://localhost:8080` | the base of the links mails carry |
//! | `MAIL_TRANSPORT` | `log` | `log` (mails are logged) or `smtp` |
//! | `SMTP_URL` | required for `smtp` | such as `smtps://user:pass@host:465` |
//! | `MAIL_FROM` | `Axum API <no-reply@localhost>` | the sender of every mail |
//! | `BREACH_CHECK` | `false` | reject passwords Have I Been Pwned knows |
//! | `TRUST_PROXY` | `false` | take the client IP from `X-Forwarded-For` |
//! | `AUTH_RATE_LIMIT` | 20 | requests per minute per IP on `/auth` (`0`: off) |
//! | `STORAGE_URL` | `file://./storage` | where uploads are stored: `file://<dir>`, `s3://<bucket>` (`AWS_*` settings) or `memory://` |
//! | `MEDIA_MAX_UPLOAD_BYTES` | 10485760 (10 MiB) | the largest `POST /media` body |
//! | `MEDIA_ALLOWED_TYPES` | `image/jpeg,image/png,image/webp,image/gif,application/pdf` | the detected types an upload may have |
//! | `DB_MAX_CONNECTIONS` | 20 | pool size |
//! | `DB_MIN_CONNECTIONS` | 5 | idle connections kept open |
//! | `DB_ACQUIRE_TIMEOUT_SECS` | 3 | wait for a connection |
//! | `DB_IDLE_TIMEOUT_SECS` | 600 | close an idle connection after |
//! | `DB_MAX_LIFETIME_SECS` | 1800 | replace a connection after |
//! | `RUST_LOG` | `info` | the log filter ([`log_filter`]) |
//!
//! A boolean is `true` or `false` (also `1` or `0`).

use std::env;
use std::str::FromStr;
use std::time::Duration;

use crate::db::pool::PoolSettings;
use crate::shared::storage::StorageUrl;

/// Where the server listens when `BIND_ADDR` is not set.
const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8080";

/// The log filter when `RUST_LOG` is not set: everything at `info` and
/// above.
const DEFAULT_LOG_FILTER: &str = "info";

/// How long a token lasts when `JWT_TTL` is not set: an hour.
const DEFAULT_JWT_TTL_SECS: u64 = 3600;

/// The shortest `JWT_SECRET` accepted, in bytes: an HS256 key shorter than
/// the hash (32 bytes) is easier to guess than the signature is to forge.
pub const MIN_JWT_SECRET_BYTES: usize = 32;

/// The link base when `APP_URL` is not set.
const DEFAULT_APP_URL: &str = "http://localhost:8080";

/// The sender when `MAIL_FROM` is not set.
const DEFAULT_MAIL_FROM: &str = "Axum API <no-reply@localhost>";

/// Requests per minute per IP on `/auth` when `AUTH_RATE_LIMIT` is not set.
const DEFAULT_AUTH_RATE_LIMIT: u32 = 20;

/// The largest upload when `MEDIA_MAX_UPLOAD_BYTES` is not set: 10 MiB.
const DEFAULT_MEDIA_MAX_UPLOAD_BYTES: usize = 10 * 1024 * 1024;

/// The detected types an upload may have when `MEDIA_ALLOWED_TYPES` is not
/// set.
const DEFAULT_MEDIA_ALLOWED_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/gif",
    "application/pdf",
];

/// The log filter: `RUST_LOG` (such as `debug` or `info,sqlx=warn`), else
/// [`DEFAULT_LOG_FILTER`]. `main.rs` reads it before [`Config::from_env`].
#[must_use]
pub fn log_filter() -> String {
    env::var("RUST_LOG").unwrap_or_else(|_| DEFAULT_LOG_FILTER.to_owned())
}

/// How mail leaves the application (`MAIL_TRANSPORT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailTransport {
    /// `log`: each mail is logged, not sent. The default, for development.
    Log,
    /// `smtp`: sent through `SMTP_URL`.
    Smtp,
}

/// The application's settings. It has no `Debug`, so the secrets can't end
/// up in a log.
pub struct Config {
    /// `DATABASE_URL`.
    pub database_url: String,
    /// `BIND_ADDR`.
    pub bind_addr: String,
    /// `JWT_SECRET`.
    pub jwt_secret: String,
    /// `JWT_TTL`: how long an issued token lasts.
    pub jwt_ttl: Duration,
    /// `REQUIRE_VERIFIED_EMAIL`: sign-in answers `403` until the email is
    /// verified.
    pub require_verified_email: bool,
    /// `APP_URL`: the base of mailed links, such as
    /// `{app_url}/verify-email?token=…`.
    pub app_url: String,
    /// `MAIL_TRANSPORT`.
    pub mail_transport: MailTransport,
    /// `SMTP_URL`, set whenever `mail_transport` is `Smtp`.
    pub smtp_url: Option<String>,
    /// `MAIL_FROM`.
    pub mail_from: String,
    /// `BREACH_CHECK`: check new passwords against Have I Been Pwned.
    pub breach_check: bool,
    /// `TRUST_PROXY`: take the client IP from `X-Forwarded-For`.
    pub trust_proxy: bool,
    /// `AUTH_RATE_LIMIT`: requests per minute per IP on `/auth`; `0` turns
    /// the limit off.
    pub auth_rate_limit: u32,
    /// `STORAGE_URL`: where uploaded files are stored.
    pub storage_url: StorageUrl,
    /// `MEDIA_MAX_UPLOAD_BYTES`: the largest upload, in bytes.
    pub media_max_upload_bytes: usize,
    /// `MEDIA_ALLOWED_TYPES`: the types, detected from the bytes, an upload
    /// may have (comma-separated).
    pub media_allowed_types: Vec<String>,
    /// The pool's size and timeouts, `DB_*`.
    pub pool: PoolSettings,
}

/// A setting missing from, or invalid in, the environment.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A required variable is not set.
    #[error("environment variable `{0}` is not set")]
    Missing(&'static str),
    /// A key is too short to resist guessing.
    #[error(
        "environment variable `{name}` is {len} bytes long; it needs at least {min} (try `openssl rand -base64 48`)"
    )]
    TooShort {
        /// The variable.
        name: &'static str,
        /// Its length, in bytes.
        len: usize,
        /// The least it may be.
        min: usize,
    },
    /// A variable is not a whole number.
    #[error("environment variable `{name}` is `{value}`, which is not a whole number")]
    NotANumber {
        /// The variable.
        name: &'static str,
        /// Its value.
        value: String,
    },
    /// A variable is none of the values it takes.
    #[error("environment variable `{name}` is `{value}`, which is not one of {expected}")]
    NotOneOf {
        /// The variable.
        name: &'static str,
        /// Its value.
        value: String,
        /// The values it takes.
        expected: &'static str,
    },
}

impl Config {
    /// Reads every setting from the environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Config::from_vars(|name| env::var(name).ok())
    }

    /// Reads every setting through `var`, which gives a variable's value
    /// when it is set.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let vars = Vars(&var);
        let mut pool = PoolSettings::default();
        if let Some(max) = vars.number("DB_MAX_CONNECTIONS")? {
            pool.max_connections = max;
        }
        if let Some(min) = vars.number("DB_MIN_CONNECTIONS")? {
            pool.min_connections = min;
        }
        if let Some(secs) = vars.number("DB_ACQUIRE_TIMEOUT_SECS")? {
            pool.acquire_timeout = Duration::from_secs(secs);
        }
        if let Some(secs) = vars.number("DB_IDLE_TIMEOUT_SECS")? {
            pool.idle_timeout = Duration::from_secs(secs);
        }
        if let Some(secs) = vars.number("DB_MAX_LIFETIME_SECS")? {
            pool.max_lifetime = Duration::from_secs(secs);
        }
        let mail_transport = match var("MAIL_TRANSPORT").as_deref() {
            None | Some("log") => MailTransport::Log,
            Some("smtp") => MailTransport::Smtp,
            Some(other) => {
                return Err(ConfigError::NotOneOf {
                    name: "MAIL_TRANSPORT",
                    value: other.to_owned(),
                    expected: "log, smtp",
                });
            }
        };
        let smtp_url = match mail_transport {
            MailTransport::Smtp => Some(vars.required("SMTP_URL")?),
            MailTransport::Log => var("SMTP_URL"),
        };
        Ok(Config {
            database_url: vars.required("DATABASE_URL")?,
            bind_addr: var("BIND_ADDR").unwrap_or_else(|| DEFAULT_BIND_ADDR.to_owned()),
            jwt_secret: vars.secret("JWT_SECRET", MIN_JWT_SECRET_BYTES)?,
            jwt_ttl: Duration::from_secs(vars.number("JWT_TTL")?.unwrap_or(DEFAULT_JWT_TTL_SECS)),
            require_verified_email: vars.flag("REQUIRE_VERIFIED_EMAIL")?.unwrap_or(true),
            app_url: var("APP_URL").unwrap_or_else(|| DEFAULT_APP_URL.to_owned()),
            mail_transport,
            smtp_url,
            mail_from: var("MAIL_FROM").unwrap_or_else(|| DEFAULT_MAIL_FROM.to_owned()),
            breach_check: vars.flag("BREACH_CHECK")?.unwrap_or(false),
            trust_proxy: vars.flag("TRUST_PROXY")?.unwrap_or(false),
            auth_rate_limit: vars
                .number("AUTH_RATE_LIMIT")?
                .unwrap_or(DEFAULT_AUTH_RATE_LIMIT),
            storage_url: match var("STORAGE_URL") {
                Some(url) => url.parse().map_err(|_| ConfigError::NotOneOf {
                    name: "STORAGE_URL",
                    value: url,
                    expected: "file://<dir>, s3://<bucket>, memory://",
                })?,
                None => StorageUrl::default_for_development(),
            },
            media_max_upload_bytes: vars
                .number("MEDIA_MAX_UPLOAD_BYTES")?
                .unwrap_or(DEFAULT_MEDIA_MAX_UPLOAD_BYTES),
            media_allowed_types: match var("MEDIA_ALLOWED_TYPES") {
                Some(list) => list
                    .split(',')
                    .map(str::trim)
                    .filter(|kind| !kind.is_empty())
                    .map(str::to_owned)
                    .collect(),
                None => DEFAULT_MEDIA_ALLOWED_TYPES
                    .iter()
                    .map(|kind| (*kind).to_owned())
                    .collect(),
            },
            pool,
        })
    }
}

/// The variables, read through a lookup.
struct Vars<'a>(&'a dyn Fn(&str) -> Option<String>);

impl Vars<'_> {
    /// A variable that must be set.
    fn required(&self, name: &'static str) -> Result<String, ConfigError> {
        (self.0)(name).ok_or(ConfigError::Missing(name))
    }

    /// A key that must be set, at least `min` bytes long.
    fn secret(&self, name: &'static str, min: usize) -> Result<String, ConfigError> {
        let value = self.required(name)?;
        if value.len() < min {
            return Err(ConfigError::TooShort {
                name,
                len: value.len(),
                min,
            });
        }
        Ok(value)
    }

    /// A whole number, when the variable is set.
    fn number<T: FromStr>(&self, name: &'static str) -> Result<Option<T>, ConfigError> {
        match (self.0)(name) {
            Some(value) => value
                .parse()
                .map(Some)
                .map_err(|_| ConfigError::NotANumber { name, value }),
            None => Ok(None),
        }
    }

    /// A boolean, when the variable is set.
    fn flag(&self, name: &'static str) -> Result<Option<bool>, ConfigError> {
        match (self.0)(name).as_deref() {
            None => Ok(None),
            Some("true" | "1") => Ok(Some(true)),
            Some("false" | "0") => Ok(Some(false)),
            Some(other) => Err(ConfigError::NotOneOf {
                name,
                value: other.to_owned(),
                expected: "true, false",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// The settings read from `vars` alone.
    fn read(vars: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        Config::from_vars(|name| vars.get(name).cloned())
    }

    const REQUIRED: &[(&str, &str)] = &[
        ("DATABASE_URL", "postgres://db"),
        ("JWT_SECRET", "a test secret of at least 32 bytes"),
    ];

    #[test]
    fn the_auth_settings_default_for_development() -> Result<(), ConfigError> {
        let config = read(REQUIRED)?;
        assert_eq!(config.jwt_ttl, Duration::from_secs(3600));
        assert!(config.require_verified_email);
        assert_eq!(config.app_url, "http://localhost:8080");
        assert_eq!(config.mail_transport, MailTransport::Log);
        assert_eq!(config.smtp_url, None);
        assert_eq!(config.mail_from, "Axum API <no-reply@localhost>");
        assert!(!config.breach_check);
        assert!(!config.trust_proxy);
        assert_eq!(config.auth_rate_limit, 20);
        Ok(())
    }

    #[test]
    fn the_auth_settings_are_read() -> Result<(), ConfigError> {
        let mut vars = REQUIRED.to_vec();
        vars.extend([
            ("JWT_TTL", "900"),
            ("REQUIRE_VERIFIED_EMAIL", "false"),
            ("APP_URL", "https://app.example.com"),
            ("MAIL_TRANSPORT", "smtp"),
            ("SMTP_URL", "smtps://u:p@mail.example.com"),
            ("MAIL_FROM", "App <hi@example.com>"),
            ("BREACH_CHECK", "1"),
            ("TRUST_PROXY", "true"),
            ("AUTH_RATE_LIMIT", "5"),
        ]);
        let config = read(&vars)?;
        assert_eq!(config.jwt_ttl, Duration::from_secs(900));
        assert!(!config.require_verified_email);
        assert_eq!(config.app_url, "https://app.example.com");
        assert_eq!(config.mail_transport, MailTransport::Smtp);
        assert_eq!(
            config.smtp_url.as_deref(),
            Some("smtps://u:p@mail.example.com")
        );
        assert_eq!(config.mail_from, "App <hi@example.com>");
        assert!(config.breach_check);
        assert!(config.trust_proxy);
        assert_eq!(config.auth_rate_limit, 5);
        Ok(())
    }

    #[test]
    fn a_jwt_secret_under_32_bytes_is_refused() {
        let error = read(&[
            ("DATABASE_URL", "postgres://db"),
            ("JWT_SECRET", "change-me"),
        ])
        .err()
        .map(|err| err.to_string());
        assert_eq!(
            error.as_deref(),
            Some(
                "environment variable `JWT_SECRET` is 9 bytes long; it needs at least 32 (try `openssl rand -base64 48`)"
            )
        );
        let secret = "x".repeat(32);
        assert!(read(&[("DATABASE_URL", "postgres://db"), ("JWT_SECRET", &secret)]).is_ok());
    }

    #[test]
    fn smtp_needs_its_url_and_values_are_checked() {
        let mut vars = REQUIRED.to_vec();
        vars.push(("MAIL_TRANSPORT", "smtp"));
        let error = read(&vars).err().map(|err| err.to_string());
        assert_eq!(
            error.as_deref(),
            Some("environment variable `SMTP_URL` is not set")
        );

        let mut vars = REQUIRED.to_vec();
        vars.push(("TRUST_PROXY", "yes"));
        let error = read(&vars).err().map(|err| err.to_string());
        assert_eq!(
            error.as_deref(),
            Some("environment variable `TRUST_PROXY` is `yes`, which is not one of true, false")
        );

        let mut vars = REQUIRED.to_vec();
        vars.push(("MAIL_TRANSPORT", "carrier-pigeon"));
        let error = read(&vars).err().map(|err| err.to_string());
        assert_eq!(
            error.as_deref(),
            Some(
                "environment variable `MAIL_TRANSPORT` is `carrier-pigeon`, which is not one of log, smtp"
            )
        );
    }

    #[test]
    fn the_media_settings_default_and_are_read() -> Result<(), ConfigError> {
        let config = read(REQUIRED)?;
        assert_eq!(config.storage_url, StorageUrl::default_for_development());
        assert_eq!(config.media_max_upload_bytes, 10 * 1024 * 1024);
        assert_eq!(
            config.media_allowed_types,
            [
                "image/jpeg",
                "image/png",
                "image/webp",
                "image/gif",
                "application/pdf"
            ]
        );

        let mut vars = REQUIRED.to_vec();
        vars.extend([
            ("STORAGE_URL", "s3://uploads"),
            ("MEDIA_MAX_UPLOAD_BYTES", "1024"),
            ("MEDIA_ALLOWED_TYPES", " image/png , application/pdf,"),
        ]);
        let config = read(&vars)?;
        assert_eq!(config.storage_url, StorageUrl::S3("uploads".to_owned()));
        assert_eq!(config.media_max_upload_bytes, 1024);
        assert_eq!(config.media_allowed_types, ["image/png", "application/pdf"]);

        let mut vars = REQUIRED.to_vec();
        vars.push(("STORAGE_URL", "ftp://files"));
        let error = read(&vars).err().map(|err| err.to_string());
        assert_eq!(
            error.as_deref(),
            Some(
                "environment variable `STORAGE_URL` is `ftp://files`, which is not one of file://<dir>, s3://<bucket>, memory://"
            )
        );
        Ok(())
    }
}
