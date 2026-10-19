// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The application's settings, read from the environment into one
//! [`Config`]. This is the only file that reads environment variables.
//!
//! | Variable | Default | Setting |
//! |---|---|---|
//! | `DATABASE_URL` | required | the PostgreSQL URL |
//! | `BIND_ADDR` | `0.0.0.0:8080` | where the server listens |
//! | `JWT_SECRET` | required | the HS256 key tokens are signed with |
//! | `DB_MAX_CONNECTIONS` | 20 | pool size |
//! | `DB_MIN_CONNECTIONS` | 5 | idle connections kept open |
//! | `DB_ACQUIRE_TIMEOUT_SECS` | 3 | wait for a connection |
//! | `DB_IDLE_TIMEOUT_SECS` | 600 | close an idle connection after |
//! | `DB_MAX_LIFETIME_SECS` | 1800 | replace a connection after |
//! | `RUST_LOG` | `info` | the log filter ([`log_filter`]) |

use std::env;
use std::str::FromStr;
use std::time::Duration;

use crate::db::pool::PoolSettings;

/// Where the server listens when `BIND_ADDR` is not set.
const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8080";

/// The log filter when `RUST_LOG` is not set: everything at `info` and
/// above.
const DEFAULT_LOG_FILTER: &str = "info";

/// The log filter: `RUST_LOG` (such as `debug` or `info,sqlx=warn`), else
/// [`DEFAULT_LOG_FILTER`]. `main.rs` reads it before [`Config::from_env`].
#[must_use]
pub fn log_filter() -> String {
    env::var("RUST_LOG").unwrap_or_else(|_| DEFAULT_LOG_FILTER.to_owned())
}

/// The application's settings. It has no `Debug`, so the secret can't end
/// up in a log.
pub struct Config {
    /// `DATABASE_URL`.
    pub database_url: String,
    /// `BIND_ADDR`.
    pub bind_addr: String,
    /// `JWT_SECRET`.
    pub jwt_secret: String,
    /// The pool's size and timeouts, `DB_*`.
    pub pool: PoolSettings,
}

/// A setting missing from, or invalid in, the environment.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A required variable is not set.
    #[error("environment variable `{0}` is not set")]
    Missing(&'static str),
    /// A variable is not a whole number.
    #[error("environment variable `{name}` is `{value}`, which is not a whole number")]
    NotANumber {
        /// The variable.
        name: &'static str,
        /// Its value.
        value: String,
    },
}

impl Config {
    /// Reads every setting from the environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        let mut pool = PoolSettings::default();
        if let Some(max) = number("DB_MAX_CONNECTIONS")? {
            pool.max_connections = max;
        }
        if let Some(min) = number("DB_MIN_CONNECTIONS")? {
            pool.min_connections = min;
        }
        if let Some(secs) = number("DB_ACQUIRE_TIMEOUT_SECS")? {
            pool.acquire_timeout = Duration::from_secs(secs);
        }
        if let Some(secs) = number("DB_IDLE_TIMEOUT_SECS")? {
            pool.idle_timeout = Duration::from_secs(secs);
        }
        if let Some(secs) = number("DB_MAX_LIFETIME_SECS")? {
            pool.max_lifetime = Duration::from_secs(secs);
        }
        Ok(Config {
            database_url: required("DATABASE_URL")?,
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_owned()),
            jwt_secret: required("JWT_SECRET")?,
            pool,
        })
    }
}

/// A variable that must be set.
fn required(name: &'static str) -> Result<String, ConfigError> {
    env::var(name).map_err(|_| ConfigError::Missing(name))
}

/// A whole number, when the variable is set.
fn number<T: FromStr>(name: &'static str) -> Result<Option<T>, ConfigError> {
    match env::var(name) {
        Ok(value) => value
            .parse()
            .map(Some)
            .map_err(|_| ConfigError::NotANumber { name, value }),
        Err(_) => Ok(None),
    }
}
