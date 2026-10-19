//! What the tests share: the JWT secret the test application verifies
//! tokens with, and callers holding a token signed with it.

#![allow(dead_code, reason = "each test file uses some of the helpers")]

use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{EncodingKey, Header, encode};
use rok_sqlc_api_example::shared::auth::{AuthUser, Claims, Role};
use uuid::Uuid;

/// What a test returns: `?` works on any error.
pub type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The secret the test application's `JwtKeys` use.
pub const SECRET: &str = "test secret";

/// A signed-in caller with a fresh id.
#[must_use]
pub fn caller() -> AuthUser {
    AuthUser {
        id: Uuid::now_v7(),
        role: Role::User,
    }
}

/// `user`'s `Authorization` header: a bearer token signed with [`SECRET`],
/// valid for an hour.
pub fn bearer(user: AuthUser) -> Result<String, Box<dyn std::error::Error>> {
    let exp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 3600;
    let claims = Claims {
        sub: user.id,
        role: user.role,
        exp,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(SECRET.as_bytes()),
    )?;
    Ok(format!("Bearer {token}"))
}
