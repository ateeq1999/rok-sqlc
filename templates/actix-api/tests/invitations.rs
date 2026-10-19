//! Inviting someone (`POST /auth/invitations`, admins only): an account
//! with no password, a "set your password" link mailed to it, and an
//! `audit_log` row, all on one transaction.

mod support;

use std::sync::Arc;

use actix_api::app::AuditLog;
use actix_api::db::audit_log_entry::read as audit_log;
use actix_api::error::AppError;
use actix_api::features::audit_log_entry::AuditLogEntryService;
use actix_api::features::auth::service::{AuditEntry, AuditTrail};
use actix_web::http::{Method, StatusCode};
use serde_json::json;
use sqlx::PgConnection;
use support::{
    Database, PASSWORD, TestResult, account, call, call_with, mailed, mailed_token, state, user,
};
use uuid::Uuid;

const INVITE: &str = "/api/v1/auth/invitations";
const RESET: &str = "/api/v1/auth/password/reset";
const LOGIN: &str = "/api/v1/auth/login";

#[actix_web::test]
async fn an_invitee_sets_a_password_with_the_link_and_signs_in_verified() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let (admin_id, admin) = account(&db.pool, actix_api::db::types::Role::Admin).await?;
    let body = json!({"email": " Guest@Example.com ", "display_name": "Guest", "role": "admin"});
    let answer = call(&db.pool, Some(&admin), Method::POST, INVITE, Some(body)).await?;
    assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.body);
    assert_eq!(answer.body["email"], "guest@example.com");
    assert_eq!(answer.body["display_name"], "Guest");
    assert_eq!(answer.body["role"], "admin");
    assert_eq!(answer.body["has_password"], false);
    assert_eq!(answer.body["email_verified"], false);
    let guest: Uuid = serde_json::from_value(answer.body["id"].clone())?;

    let entries = audit_log::list(&db.pool).await?;
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].action, "user.invited");
    assert_eq!(Uuid::from(entries[0].actor_user_id), admin_id);
    assert_eq!(entries[0].target_user_id.map(Uuid::from), Some(guest));

    let mail = mailed(&db.pool).await?;
    assert_eq!(mail.len(), 1, "{mail:?}");
    assert_eq!(mail[0].to, "guest@example.com");
    let link = mailed_token(&mail[0]).ok_or("no invitation link")?;

    // The invitee sets a password with the link, which verifies the email,
    // and signs in.
    let body = json!({"token": link, "new_password": PASSWORD});
    let answer = call(&db.pool, None, Method::POST, RESET, Some(body)).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let credentials = json!({"email": "guest@example.com", "password": PASSWORD});
    let answer = call(&db.pool, None, Method::POST, LOGIN, Some(credentials)).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let session = answer.body["access_token"].as_str().ok_or("no session")?;
    let answer = call(
        &db.pool,
        Some(session),
        Method::GET,
        "/api/v1/auth/me",
        None,
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(answer.body["id"], guest.to_string());
    assert_eq!(answer.body["email_verified"], true);
    assert_eq!(answer.body["has_password"], true);
    db.drop().await
}

#[actix_web::test]
async fn only_an_admin_invites_and_a_taken_email_is_a_conflict() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let body = json!({"email": "guest@example.com"});
    let answer = call(&db.pool, None, Method::POST, INVITE, Some(body.clone())).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    let token = user(&db.pool).await?;
    let answer = call(
        &db.pool,
        Some(&token),
        Method::POST,
        INVITE,
        Some(body.clone()),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.body);

    let admin = support::admin(&db.pool).await?;
    let answer = call(&db.pool, Some(&admin), Method::POST, INVITE, Some(body)).await?;
    assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.body);
    assert_eq!(answer.body["role"], "user");
    let again = json!({"email": "GUEST@example.com"});
    let answer = call(&db.pool, Some(&admin), Method::POST, INVITE, Some(again)).await?;
    assert_eq!(answer.status, StatusCode::CONFLICT, "{}", answer.body);

    let answer = call(
        &db.pool,
        Some(&admin),
        Method::POST,
        INVITE,
        Some(json!({"email": "not an address"})),
    )
    .await?;
    assert_eq!(
        answer.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        answer.body
    );
    assert_eq!(audit_log::list(&db.pool).await?.len(), 1);
    assert_eq!(mailed(&db.pool).await?.len(), 1);
    db.drop().await
}

/// The real audit log, but failing once its entry is written: the entry
/// and everything else on the flow's transaction must roll back.
struct Broken(AuditLog);

#[async_trait::async_trait]
impl AuditTrail for Broken {
    async fn record_in(&self, conn: &mut PgConnection, entry: AuditEntry) -> Result<(), AppError> {
        self.0.record_in(conn, entry).await?;
        Err(AppError::Internal("the audit log is down".into()))
    }
}

#[actix_web::test]
async fn a_failed_audit_write_rolls_the_invitation_back() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let admin = support::admin(&db.pool).await?;
    let mut broken = state(&db.pool)?;
    broken.audit = Arc::new(Broken(AuditLog(AuditLogEntryService::new(db.pool.clone()))));
    let body = json!({"email": "guest@example.com"});
    let answer = call_with(
        &broken,
        Some(&admin),
        Method::POST,
        INVITE,
        Some(body.clone()),
    )
    .await?;
    assert_eq!(
        answer.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{}",
        answer.body
    );
    assert!(mailed(&db.pool).await?.is_empty());
    assert!(audit_log::list(&db.pool).await?.is_empty());

    // No account was left behind: the address is still free.
    let answer = call(&db.pool, Some(&admin), Method::POST, INVITE, Some(body)).await?;
    assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.body);
    db.drop().await
}
