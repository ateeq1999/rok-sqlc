//! `AuthUser` and `AdminUser` check the account behind a valid token on
//! every request: a disabled, deleted or signed-out account is a `401`, and
//! the role comes from the database, so a change applies without signing in
//! again.

mod support;

use axum::http::{Method, StatusCode};
use axum_api::db::types::Role;
use support::{Database, TestResult, account, call, token};

/// An admin-only route: `AdminUser` answers `401` or `403` before it runs.
const ADMIN_ROUTE: &str = "/api/v1/users";

#[tokio::test]
async fn a_disabled_accounts_valid_token_is_unauthorized() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let (id, token) = account(&db.pool, Role::Admin).await?;
    let answer = call(&db.pool, Some(&token), Method::GET, ADMIN_ROUTE, None).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);

    sqlx::query("UPDATE users SET is_active = false WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await?;
    let answer = call(&db.pool, Some(&token), Method::GET, ADMIN_ROUTE, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    assert_eq!(answer.code(), "unauthorized");
    db.drop().await
}

#[tokio::test]
async fn a_deleted_or_unknown_accounts_token_is_unauthorized() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let (id, token) = account(&db.pool, Role::Admin).await?;
    sqlx::query("UPDATE users SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await?;
    let answer = call(&db.pool, Some(&token), Method::GET, ADMIN_ROUTE, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);

    let stranger = support::token(uuid::Uuid::now_v7(), Role::Admin, 1)?;
    let answer = call(&db.pool, Some(&stranger), Method::GET, ADMIN_ROUTE, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    db.drop().await
}

#[tokio::test]
async fn a_token_issued_before_a_token_version_bump_is_unauthorized() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let (id, stale) = account(&db.pool, Role::Admin).await?;
    sqlx::query("UPDATE users SET token_version = token_version + 1 WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await?;
    let answer = call(&db.pool, Some(&stale), Method::GET, ADMIN_ROUTE, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);

    let fresh = token(id, Role::Admin, 2)?;
    let answer = call(&db.pool, Some(&fresh), Method::GET, ADMIN_ROUTE, None).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    db.drop().await
}

#[tokio::test]
async fn a_role_change_applies_without_signing_in_again() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let (id, token) = account(&db.pool, Role::User).await?;
    let answer = call(&db.pool, Some(&token), Method::GET, ADMIN_ROUTE, None).await?;
    assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.body);

    let set_role = |role: Role| {
        sqlx::query("UPDATE users SET role = $1 WHERE id = $2")
            .bind(role)
            .bind(id)
    };
    set_role(Role::Admin).execute(&db.pool).await?;
    let answer = call(&db.pool, Some(&token), Method::GET, ADMIN_ROUTE, None).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);

    set_role(Role::User).execute(&db.pool).await?;
    let answer = call(&db.pool, Some(&token), Method::GET, ADMIN_ROUTE, None).await?;
    assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.body);
    db.drop().await
}

#[tokio::test]
async fn no_token_or_a_forged_one_is_unauthorized() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let answer = call(&db.pool, None, Method::GET, ADMIN_ROUTE, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    let answer = call(&db.pool, Some("not.a.jwt"), Method::GET, ADMIN_ROUTE, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    db.drop().await
}
