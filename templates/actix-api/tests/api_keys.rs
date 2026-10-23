//! API keys: `GET`/`POST /api-keys` and `DELETE /api-keys/{id}`, and a key
//! standing in for a session on every other `AuthUser` route, sent as
//! `Authorization: Bearer rok_…` or `X-API-Key`.

mod support;

use actix_web::http::{Method, StatusCode};
use serde_json::{Value, json};
use support::{Answer, Database, TestResult, call, call_headers, user};

const KEYS: &str = "/api/v1/api-keys";
const ME: &str = "/api/v1/users/me";

/// Creates a key named `name` with `scope` for the caller of `token`: the
/// `201` answer's body.
async fn create(
    pool: &sqlx::PgPool,
    token: &str,
    name: &str,
    scope: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let body = Some(json!({"name": name, "scope": scope}));
    let answer = call(pool, Some(token), Method::POST, KEYS, body).await?;
    assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.body);
    Ok(answer.body)
}

/// The raw key of a created key's body.
fn raw(created: &Value) -> &str {
    created["key"].as_str().unwrap_or_default()
}

/// Sends `method uri` with `key` as the `X-API-Key` header.
async fn with_header(
    pool: &sqlx::PgPool,
    key: &str,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> Result<Answer, Box<dyn std::error::Error>> {
    call_headers(pool, &[("x-api-key", key)], method, uri, body).await
}

#[actix_web::test]
async fn a_key_is_shown_once_and_works_as_a_bearer_token_or_a_header() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let token = user(&db.pool).await?;
    let created = create(&db.pool, &token, "ci", "read").await?;
    let key = raw(&created);
    assert_eq!(key.len(), "rok_".len() + 8 + 1 + 32, "{key}");
    assert!(key.starts_with("rok_"), "{key}");
    assert_eq!(created["key_prefix"], key[..12]);
    assert_eq!(created["name"], "ci");
    assert_eq!(created["scope"], "read");
    assert_eq!(created["last_used_at"], Value::Null);

    let me = call(&db.pool, Some(&token), Method::GET, ME, None).await?;
    let answer = call(&db.pool, Some(key), Method::GET, ME, None).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(answer.body, me.body);
    let answer = with_header(&db.pool, key, Method::GET, ME, None).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(answer.body, me.body);

    let listed = call(&db.pool, Some(&token), Method::GET, KEYS, None).await?;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    let listed = listed.body.as_array().cloned().unwrap_or_default();
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0]["id"], created["id"]);
    assert!(listed[0]["last_used_at"].is_string(), "{:?}", listed[0]);
    assert!(listed[0].get("key").is_none(), "{:?}", listed[0]);
    db.drop().await
}

#[actix_web::test]
async fn a_read_key_may_only_read_and_a_write_key_may_write() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let token = user(&db.pool).await?;
    let body = json!({"display_name": "Ada"});
    let read = create(&db.pool, &token, "read", "read").await?;
    let answer = call(
        &db.pool,
        Some(raw(&read)),
        Method::PATCH,
        ME,
        Some(body.clone()),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.body);
    assert_eq!(answer.code(), "forbidden");

    let write = create(&db.pool, &token, "write", "write").await?;
    let answer = with_header(&db.pool, raw(&write), Method::PATCH, ME, Some(body)).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(answer.body["display_name"], "Ada");
    db.drop().await
}

#[actix_web::test]
async fn no_key_reaches_auth_or_the_keys_themselves() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let token = user(&db.pool).await?;
    let created = create(&db.pool, &token, "everything", "write").await?;
    let key = raw(&created);
    for uri in ["/api/v1/auth/me", KEYS] {
        let answer = call(&db.pool, Some(key), Method::GET, uri, None).await?;
        assert_eq!(
            answer.status,
            StatusCode::FORBIDDEN,
            "{uri}: {}",
            answer.body
        );
        let answer = with_header(&db.pool, key, Method::GET, uri, None).await?;
        assert_eq!(
            answer.status,
            StatusCode::FORBIDDEN,
            "{uri}: {}",
            answer.body
        );
    }
    let body = Some(json!({"name": "another", "scope": "write"}));
    let answer = call(&db.pool, Some(key), Method::POST, KEYS, body).await?;
    assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.body);
    let uri = format!("{KEYS}/{}", created["id"].as_str().unwrap_or_default());
    let answer = call(&db.pool, Some(key), Method::DELETE, &uri, None).await?;
    assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.body);
    db.drop().await
}

#[actix_web::test]
async fn an_encoded_path_does_not_let_a_key_reach_auth_or_the_keys() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let token = user(&db.pool).await?;
    let created = create(&db.pool, &token, "everything", "write").await?;
    let key = raw(&created);
    let id = created["id"].as_str().unwrap_or_default();
    let body = || Some(json!({"name": "another", "scope": "write"}));
    let attempts = [
        (Method::GET, "/api/v1/api%2Dkeys".to_owned(), None),
        (Method::POST, "/api/v1/api%2Dkeys".to_owned(), body()),
        (Method::POST, "/api/v1/%61pi-keys".to_owned(), body()),
        (Method::DELETE, format!("/api/v1/api%2dkeys/{id}"), None),
        (Method::GET, "/api/v1/%61uth/me".to_owned(), None),
        (Method::GET, "/api/v1/au%74h/me".to_owned(), None),
    ];
    for (method, uri, body) in attempts {
        let answer = call(&db.pool, Some(key), method.clone(), &uri, body).await?;
        assert!(
            matches!(answer.status, StatusCode::FORBIDDEN | StatusCode::NOT_FOUND),
            "{method} {uri}: {} {}",
            answer.status,
            answer.body
        );
    }
    let listed = call(&db.pool, Some(&token), Method::GET, KEYS, None).await?;
    assert_eq!(
        listed.body.as_array().map(Vec::len),
        Some(1),
        "{}",
        listed.body
    );
    db.drop().await
}

#[actix_web::test]
async fn an_expired_or_revoked_key_or_one_of_a_disabled_account_is_refused() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let token = user(&db.pool).await?;

    let body = Some(json!({"name": "short", "scope": "read", "expires_in_days": 1}));
    let answer = call(&db.pool, Some(&token), Method::POST, KEYS, body).await?;
    assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.body);
    assert!(answer.body["expires_at"].is_string(), "{}", answer.body);
    let expiring = raw(&answer.body).to_owned();
    let answer = call(&db.pool, Some(&expiring), Method::GET, ME, None).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    sqlx::query("UPDATE api_keys SET expires_at = now() - make_interval(secs => 1)")
        .execute(&db.pool)
        .await?;
    let answer = call(&db.pool, Some(&expiring), Method::GET, ME, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);

    let revoked = create(&db.pool, &token, "revoked", "read").await?;
    let uri = format!("{KEYS}/{}", revoked["id"].as_str().unwrap_or_default());
    let answer = call(&db.pool, Some(&token), Method::DELETE, &uri, None).await?;
    assert_eq!(answer.status, StatusCode::NO_CONTENT, "{}", answer.body);
    let answer = call(&db.pool, Some(raw(&revoked)), Method::GET, ME, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    let answer = call(&db.pool, Some(&token), Method::DELETE, &uri, None).await?;
    assert_eq!(answer.status, StatusCode::NOT_FOUND, "{}", answer.body);

    // Neither shows in the list.
    let listed = call(&db.pool, Some(&token), Method::GET, KEYS, None).await?;
    assert_eq!(listed.body, json!([]));

    let live = create(&db.pool, &token, "live", "read").await?;
    let answer = with_header(&db.pool, raw(&live), Method::GET, ME, None).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    sqlx::query("UPDATE users SET is_active = false")
        .execute(&db.pool)
        .await?;
    let answer = with_header(&db.pool, raw(&live), Method::GET, ME, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);

    let answer = with_header(&db.pool, "rok_unknown_key", Method::GET, ME, None).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{}", answer.body);
    db.drop().await
}

#[actix_web::test]
async fn another_users_key_is_not_found() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let owner = user(&db.pool).await?;
    let other = user(&db.pool).await?;
    let created = create(&db.pool, &owner, "mine", "read").await?;
    let uri = format!("{KEYS}/{}", created["id"].as_str().unwrap_or_default());
    let answer = call(&db.pool, Some(&other), Method::DELETE, &uri, None).await?;
    assert_eq!(answer.status, StatusCode::NOT_FOUND, "{}", answer.body);
    let listed = call(&db.pool, Some(&other), Method::GET, KEYS, None).await?;
    assert_eq!(listed.body, json!([]));
    let answer = call(&db.pool, Some(raw(&created)), Method::GET, ME, None).await?;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    db.drop().await
}

#[actix_web::test]
async fn the_26th_active_key_is_a_conflict() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let token = user(&db.pool).await?;
    let mut first = Value::Null;
    for n in 0..25 {
        let created = create(&db.pool, &token, &format!("key {n}"), "read").await?;
        if n == 0 {
            first = created;
        }
    }
    let body = Some(json!({"name": "one too many"}));
    let answer = call(&db.pool, Some(&token), Method::POST, KEYS, body.clone()).await?;
    assert_eq!(answer.status, StatusCode::CONFLICT, "{}", answer.body);
    assert_eq!(answer.code(), "conflict");

    let listed = call(&db.pool, Some(&token), Method::GET, KEYS, None).await?;
    let listed = listed.body.as_array().cloned().unwrap_or_default();
    assert_eq!(listed.len(), 25);
    assert_eq!(listed[0]["name"], "key 24", "newest first");

    let uri = format!("{KEYS}/{}", first["id"].as_str().unwrap_or_default());
    let answer = call(&db.pool, Some(&token), Method::DELETE, &uri, None).await?;
    assert_eq!(answer.status, StatusCode::NO_CONTENT, "{}", answer.body);
    let answer = call(&db.pool, Some(&token), Method::POST, KEYS, body).await?;
    assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.body);
    assert_eq!(answer.body["scope"], "read", "the default scope");
    db.drop().await
}

#[actix_web::test]
async fn a_name_or_expiry_out_of_bounds_is_refused() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let token = user(&db.pool).await?;
    for body in [
        json!({"name": "  "}),
        json!({"name": "x".repeat(101)}),
        json!({"name": "ci", "expires_in_days": 0}),
        json!({"name": "ci", "expires_in_days": 366}),
    ] {
        let answer = call(
            &db.pool,
            Some(&token),
            Method::POST,
            KEYS,
            Some(body.clone()),
        )
        .await?;
        assert_eq!(
            answer.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}: {}",
            answer.body
        );
    }
    db.drop().await
}

#[actix_web::test]
async fn only_the_keys_hash_is_stored() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let token = user(&db.pool).await?;
    let created = create(&db.pool, &token, "ci", "write").await?;
    let key = raw(&created);
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT key_prefix, key_hash, row_to_json(k)::text FROM api_keys k")
            .fetch_all(&db.pool)
            .await?;
    assert_eq!(rows.len(), 1);
    let (prefix, hash, whole) = &rows[0];
    assert_eq!(prefix, &key[..12]);
    assert_eq!(hash.len(), 64, "a SHA-256 in hex: {hash}");
    assert_ne!(hash, key);
    assert!(!whole.contains(key), "{whole}");
    db.drop().await
}
