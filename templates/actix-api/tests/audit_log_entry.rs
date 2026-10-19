//! `GET /api/v1/audit-log`, admin only and read-only: the list's happy path
//! and one failure.

mod support;

use actix_api::db::audit_log_entry::models::NewAuditLogEntry;
use actix_api::db::audit_log_entry::write as entries;
use actix_api::db::users::models::NewUser;
use actix_api::db::users::write as users;
use actix_web::http::{Method, StatusCode};
use serde_json::json;
use support::{Database, TestResult, admin, call, user};

#[actix_web::test]
async fn an_admin_lists_the_log_newest_first() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let actor = users::create(
        &db.pool,
        NewUser {
            id: None,
            email: "root@example.com".to_owned(),
            display_name: None,
            role: None,
            is_active: None,
        },
    )
    .await?;
    let older = entries::create(
        &db.pool,
        NewAuditLogEntry {
            id: None,
            actor_user_id: actor.id,
            target_user_id: Some(actor.id),
            action: "user.update".to_owned(),
            details: Some(json!({"role": "admin"})),
        },
    )
    .await?;
    sqlx::query("UPDATE audit_log SET created_at = now() - interval '1 hour' WHERE id = $1")
        .bind(older.id)
        .execute(&db.pool)
        .await?;
    entries::create(
        &db.pool,
        NewAuditLogEntry {
            id: None,
            actor_user_id: actor.id,
            target_user_id: None,
            action: "settings.change".to_owned(),
            details: None,
        },
    )
    .await?;

    let answer = call(
        &db.pool,
        Some(&admin()?),
        Method::GET,
        "/api/v1/audit-log",
        None,
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(answer.body["total"], 2);
    let items = &answer.body["items"];
    assert_eq!(items[0]["action"], "settings.change");
    assert_eq!(items[0]["details"], json!({}));
    assert_eq!(items[0]["target_user_id"], serde_json::Value::Null);
    assert_eq!(items[1]["action"], "user.update");
    assert_eq!(items[1]["details"], json!({"role": "admin"}));
    assert_eq!(items[1]["actor_user_id"], json!(actor.id));
    db.drop().await
}

#[actix_web::test]
async fn a_user_who_is_not_an_admin_may_not_read_the_log() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let answer = call(
        &db.pool,
        Some(&user()?),
        Method::GET,
        "/api/v1/audit-log",
        None,
    )
    .await?;
    assert_eq!(answer.status, StatusCode::FORBIDDEN);
    assert_eq!(answer.code(), "forbidden");
    db.drop().await
}
