//! Changing the signed-in account's email (`POST /auth/email/change`): the
//! current password, then a link mailed to the new address
//! (`POST /auth/email/change/confirm`) and a notice to the old one.

mod support;

use actix_web::http::{Method, StatusCode};
use serde_json::json;
use support::{Database, PASSWORD, TestResult, call, mailed, mailed_token, register_verified};

const CHANGE: &str = "/api/v1/auth/email/change";
const CONFIRM: &str = "/api/v1/auth/email/change/confirm";
const LOGIN: &str = "/api/v1/auth/login";

#[actix_web::test]
async fn a_confirmed_change_moves_sign_in_to_the_new_address() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let token = register_verified(&db.pool, "old@example.com", PASSWORD).await?;
    let body = json!({"current_password": PASSWORD, "new_email": " New@Example.com "});
    let answer = call(&db.pool, Some(&token), Method::POST, CHANGE, Some(body)).await?;
    assert_eq!(answer.status, StatusCode::ACCEPTED, "{}", answer.body);

    let mail = mailed(&db.pool).await?;
    let to: Vec<&str> = mail.iter().map(|mail| mail.to.as_str()).collect();
    assert_eq!(to.len(), 2, "{to:?}");
    assert!(to.contains(&"new@example.com"), "{to:?}");
    assert!(to.contains(&"old@example.com"), "{to:?}");
    let notice = mail.iter().find(|mail| mail.to == "old@example.com");
    assert!(
        notice.is_some_and(|notice| mailed_token(notice).is_none()),
        "the notice carries no link"
    );
    let link = mail
        .iter()
        .find(|mail| mail.to == "new@example.com")
        .and_then(mailed_token)
        .ok_or("no confirmation link")?;

    let answer = call(
        &db.pool,
        None,
        Method::POST,
        CONFIRM,
        Some(json!({"token": link})),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert!(answer.body["message"].is_string(), "{}", answer.body);

    let new = json!({"email": "new@example.com", "password": PASSWORD});
    let answer = call(&db.pool, None, Method::POST, LOGIN, Some(new)).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let old = json!({"email": "old@example.com", "password": PASSWORD});
    let answer = call(&db.pool, None, Method::POST, LOGIN, Some(old)).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);

    // The link is spent.
    let answer = call(
        &db.pool,
        None,
        Method::POST,
        CONFIRM,
        Some(json!({"token": link})),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "invalid_token");
    db.drop().await
}

#[actix_web::test]
async fn a_change_to_a_taken_address_notifies_the_old_one_alone() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    register_verified(&db.pool, "taken@example.com", PASSWORD).await?;
    let token = register_verified(&db.pool, "mover@example.com", PASSWORD).await?;
    let body = json!({"current_password": PASSWORD, "new_email": "TAKEN@example.com"});
    let answer = call(&db.pool, Some(&token), Method::POST, CHANGE, Some(body)).await?;
    assert_eq!(answer.status, StatusCode::ACCEPTED, "{}", answer.body);
    // The old address hears of every request, free address or not, so the
    // notice tells nobody who has an account; no link leaves.
    let mail = mailed(&db.pool).await?;
    let to: Vec<&str> = mail.iter().map(|mail| mail.to.as_str()).collect();
    assert_eq!(to, ["mover@example.com"], "{mail:?}");
    assert!(
        mail.iter().all(|mail| mailed_token(mail).is_none()),
        "{mail:?}"
    );
    db.drop().await
}

#[actix_web::test]
async fn a_wrong_current_password_or_a_bad_address_is_refused() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let token = register_verified(&db.pool, "me@example.com", PASSWORD).await?;
    let body = json!({"current_password": "not the password", "new_email": "new@example.com"});
    let answer = call(&db.pool, Some(&token), Method::POST, CHANGE, Some(body)).await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "wrong_password");

    let body = json!({"current_password": PASSWORD, "new_email": "not an address"});
    let answer = call(&db.pool, Some(&token), Method::POST, CHANGE, Some(body)).await?;
    assert_eq!(
        answer.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        answer.body
    );
    assert!(
        answer.body["error"]["details"]["new_email"].is_array(),
        "{}",
        answer.body
    );

    let body = json!({"current_password": PASSWORD, "new_email": "new@example.com"});
    let answer = call(&db.pool, None, Method::POST, CHANGE, Some(body)).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    assert!(mailed(&db.pool).await?.is_empty());
    db.drop().await
}

#[actix_web::test]
async fn a_confirmation_after_the_address_was_taken_meanwhile_is_an_invalid_token() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let token = register_verified(&db.pool, "first@example.com", PASSWORD).await?;
    let body = json!({"current_password": PASSWORD, "new_email": "wanted@example.com"});
    let answer = call(&db.pool, Some(&token), Method::POST, CHANGE, Some(body)).await?;
    assert_eq!(answer.status, StatusCode::ACCEPTED, "{}", answer.body);
    let link = mailed(&db.pool)
        .await?
        .iter()
        .find(|mail| mail.to == "wanted@example.com")
        .and_then(mailed_token)
        .ok_or("no confirmation link")?;

    register_verified(&db.pool, "Wanted@Example.com", PASSWORD).await?;
    let answer = call(
        &db.pool,
        None,
        Method::POST,
        CONFIRM,
        Some(json!({"token": link})),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.code(), "invalid_token");

    let mine = json!({"email": "first@example.com", "password": PASSWORD});
    let answer = call(&db.pool, None, Method::POST, LOGIN, Some(mine)).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    db.drop().await
}

#[actix_web::test]
async fn a_newer_request_spends_the_older_link() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let token = register_verified(&db.pool, "me@example.com", PASSWORD).await?;
    let mut links = Vec::new();
    for address in ["one@example.com", "two@example.com"] {
        let body = json!({"current_password": PASSWORD, "new_email": address});
        let answer = call(&db.pool, Some(&token), Method::POST, CHANGE, Some(body)).await?;
        assert_eq!(answer.status, StatusCode::ACCEPTED, "{}", answer.body);
        let link = mailed(&db.pool)
            .await?
            .iter()
            .find(|mail| mail.to == address)
            .and_then(mailed_token)
            .ok_or("no confirmation link")?;
        links.push(link);
    }
    let answer = call(
        &db.pool,
        None,
        Method::POST,
        CONFIRM,
        Some(json!({"token": links[0]})),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    let answer = call(
        &db.pool,
        None,
        Method::POST,
        CONFIRM,
        Some(json!({"token": links[1]})),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    db.drop().await
}
