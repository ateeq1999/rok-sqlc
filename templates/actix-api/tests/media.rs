//! Uploads (`/media`): a raw body stored through the `Storage` port, its
//! type detected from the bytes, each file its owner's alone.

mod support;

use std::sync::Arc;

use actix_web::http::{Method, StatusCode};
use sqlx::AssertSqlSafe;
use support::{Database, Raw, Recorded, TestResult, send, state, state_with, user};

const MEDIA: &str = "/api/v1/media";

/// A PNG's first bytes: enough for type detection.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0";

/// A PDF's first bytes.
const PDF: &[u8] = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n1 0 obj\n<<>>\nendobj\n";

/// A Windows executable's first bytes.
const EXE: &[u8] = b"MZ\x90\0\x03\0\0\0\x04\0\0\0\xff\xff\0\0";

/// `POST /media` of `bytes` with `headers`.
async fn upload(
    state: &actix_api::app::AppState,
    token: &str,
    uri: &str,
    headers: &[(&str, &str)],
    bytes: &[u8],
) -> Result<Raw, Box<dyn std::error::Error>> {
    send(
        state,
        Some(token),
        Method::POST,
        uri,
        headers,
        bytes.to_vec(),
    )
    .await
}

/// `method uri` with no body.
async fn fetch(
    state: &actix_api::app::AppState,
    token: &str,
    method: Method,
    uri: &str,
) -> Result<Raw, Box<dyn std::error::Error>> {
    send(state, Some(token), method, uri, &[], Vec::new()).await
}

#[actix_web::test]
async fn a_png_is_stored_listed_served_and_deleted() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let state = state(&db.pool)?;
    let token = user(&db.pool).await?;
    let headers = [("content-type", "application/octet-stream")];
    let answer = upload(
        &state,
        &token,
        "/api/v1/media?filename=dot.png",
        &headers,
        PNG,
    )
    .await?;
    assert_eq!(answer.status, StatusCode::CREATED);
    let created = answer.json()?;
    let id = created["id"].as_str().ok_or("no id")?.to_owned();
    assert_eq!(created["content_type"], "image/png");
    assert_eq!(created["size_bytes"], PNG.len());
    assert_eq!(created["original_filename"], "dot.png");
    assert_eq!(created["url"], format!("/api/v1/media/{id}/content"));

    let answer = fetch(&state, &token, Method::GET, &format!("{MEDIA}/{id}")).await?;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.json()?, created);

    let answer = fetch(
        &state,
        &token,
        Method::GET,
        &format!("{MEDIA}/{id}/content"),
    )
    .await?;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.bytes, PNG);
    assert_eq!(answer.header("content-type"), Some("image/png"));
    assert_eq!(answer.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(
        answer.header("content-disposition"),
        Some("inline; filename=\"dot.png\"")
    );

    let answer = fetch(&state, &token, Method::GET, MEDIA).await?;
    assert_eq!(answer.status, StatusCode::OK);
    let page = answer.json()?;
    assert_eq!(page["total"], 1);
    assert_eq!(page["page"], 1);
    assert_eq!(page["items"][0], created);

    let answer = fetch(&state, &token, Method::DELETE, &format!("{MEDIA}/{id}")).await?;
    assert_eq!(answer.status, StatusCode::NO_CONTENT);
    assert!(answer.bytes.is_empty());
    for uri in [format!("{MEDIA}/{id}"), format!("{MEDIA}/{id}/content")] {
        let answer = fetch(&state, &token, Method::GET, &uri).await?;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(answer.json()?["error"]["code"], "not_found");
    }
    let answer = fetch(&state, &token, Method::DELETE, &format!("{MEDIA}/{id}")).await?;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
    db.drop().await
}

#[actix_web::test]
async fn the_list_is_newest_first_and_paginated() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let state = state(&db.pool)?;
    let token = user(&db.pool).await?;
    let mut ids = Vec::new();
    for bytes in [PNG, PDF] {
        let created = upload(&state, &token, MEDIA, &[], bytes).await?.json()?;
        ids.push(created["id"].clone());
    }
    let page = fetch(&state, &token, Method::GET, &format!("{MEDIA}?per_page=1")).await?;
    let page = page.json()?;
    assert_eq!(page["total"], 2);
    assert_eq!(page["per_page"], 1);
    assert_eq!(page["items"][0]["id"], ids[1]);
    let page = fetch(
        &state,
        &token,
        Method::GET,
        &format!("{MEDIA}?per_page=1&page=2"),
    )
    .await?;
    assert_eq!(page.json()?["items"][0]["id"], ids[0]);
    db.drop().await
}

#[actix_web::test]
async fn a_body_over_the_limit_is_413_and_one_under_it_is_stored() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let small = state_with(&db.pool, &[("MEDIA_MAX_UPLOAD_BYTES", "1024")])?;
    let token = user(&db.pool).await?;
    let mut big = PNG.to_vec();
    big.resize(1025, 0);
    let answer = upload(&small, &token, MEDIA, &[], &big).await?;
    assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(answer.json()?["error"]["code"], "payload_too_large");
    big.truncate(1024);
    let answer = upload(&small, &token, MEDIA, &[], &big).await?;
    assert_eq!(answer.status, StatusCode::CREATED, "{:?}", answer.json());

    // The default limit, 10 MiB, is above the 2 MiB a JSON body may have.
    let state = state(&db.pool)?;
    big.resize(3 * 1024 * 1024, 0);
    let answer = upload(&state, &token, MEDIA, &[], &big).await?;
    assert_eq!(answer.status, StatusCode::CREATED, "{:?}", answer.json());
    db.drop().await
}

#[actix_web::test]
async fn an_unrecognized_or_unallowed_type_is_415() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let state = state(&db.pool)?;
    let token = user(&db.pool).await?;
    let plain_text: &[u8] = b"just some words";
    for bytes in [EXE, plain_text] {
        let answer = upload(&state, &token, MEDIA, &[], bytes).await?;
        assert_eq!(answer.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(answer.json()?["error"]["code"], "unsupported_media_type");
    }
    let answer = upload(&state, &token, MEDIA, &[], b"").await?;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    db.drop().await
}

#[actix_web::test]
async fn the_type_the_client_claims_is_ignored() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let state = state(&db.pool)?;
    let token = user(&db.pool).await?;
    let headers = [
        ("content-type", "application/pdf"),
        ("x-filename", "report.pdf"),
    ];
    let answer = upload(&state, &token, MEDIA, &headers, PNG).await?;
    assert_eq!(answer.status, StatusCode::CREATED);
    let created = answer.json()?;
    assert_eq!(created["content_type"], "image/png");
    assert_eq!(created["original_filename"], "report.pdf");
    db.drop().await
}

#[actix_web::test]
async fn a_file_name_over_255_characters_is_422() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let state = state(&db.pool)?;
    let token = user(&db.pool).await?;
    let name = "a".repeat(256);
    let answer = upload(&state, &token, MEDIA, &[("x-filename", &name)], PNG).await?;
    assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY);
    db.drop().await
}

#[actix_web::test]
async fn another_users_file_is_404() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let state = state(&db.pool)?;
    let owner = user(&db.pool).await?;
    let other = user(&db.pool).await?;
    let created = upload(&state, &owner, MEDIA, &[], PNG).await?.json()?;
    let id = created["id"].as_str().ok_or("no id")?;
    for (method, uri) in [
        (Method::GET, format!("{MEDIA}/{id}")),
        (Method::GET, format!("{MEDIA}/{id}/content")),
        (Method::DELETE, format!("{MEDIA}/{id}")),
    ] {
        let answer = fetch(&state, &other, method, &uri).await?;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{uri}");
    }
    let page = fetch(&state, &other, Method::GET, MEDIA).await?.json()?;
    assert_eq!(page["total"], 0);
    let answer = fetch(&state, &owner, Method::GET, &format!("{MEDIA}/{id}")).await?;
    assert_eq!(answer.status, StatusCode::OK);
    let answer = send(&state, None, Method::GET, MEDIA, &[], Vec::new()).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    db.drop().await
}

#[actix_web::test]
async fn a_failed_insert_leaves_no_object_behind() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let storage = Recorded::new();
    let state = state(&db.pool)?.with_storage(Arc::new(storage.clone()));
    let token = user(&db.pool).await?;
    let created = upload(&state, &token, MEDIA, &[], PNG).await?;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(storage.keys().len(), 1);

    sqlx::raw_sql(AssertSqlSafe(
        "CREATE FUNCTION refuse() RETURNS trigger LANGUAGE plpgsql AS \
         $$ BEGIN RAISE EXCEPTION 'refused'; END $$; \
         CREATE TRIGGER refuse BEFORE INSERT ON media \
         FOR EACH ROW EXECUTE FUNCTION refuse();",
    ))
    .execute(&db.pool)
    .await?;
    let answer = upload(&state, &token, MEDIA, &[], PNG).await?;
    assert_eq!(answer.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(storage.keys().len(), 1, "{:?}", storage.keys());
    db.drop().await
}

#[actix_web::test]
async fn a_pdf_is_sent_as_an_attachment() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let state = state(&db.pool)?;
    let token = user(&db.pool).await?;
    let uri = "/api/v1/media?filename=annual%20report.pdf";
    let created = upload(&state, &token, uri, &[], PDF).await?.json()?;
    assert_eq!(created["content_type"], "application/pdf");
    let url = created["url"].as_str().ok_or("no url")?;
    let answer = fetch(&state, &token, Method::GET, url).await?;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.bytes, PDF);
    assert_eq!(answer.header("content-type"), Some("application/pdf"));
    assert_eq!(answer.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(
        answer.header("content-disposition"),
        Some("attachment; filename=\"annual_report.pdf\"")
    );
    db.drop().await
}

#[actix_web::test]
async fn the_state_stores_uploads_where_storage_url_says() -> TestResult {
    let dir = format!("target/test-storage-{}", uuid::Uuid::now_v7().simple());
    let url = format!("file://./{dir}");
    let pool = sqlx::PgPool::connect_lazy("postgres://localhost/unused")?;
    let config = support::config(&[("STORAGE_URL", &url)])?;
    let state = actix_api::app::AppState::new(pool, &config)?;
    state
        .storage
        .put(
            "probe/one.txt",
            bytes::Bytes::from_static(b"stored"),
            "text/plain",
        )
        .await?;
    let stored = std::fs::read(std::path::Path::new(&dir).join("probe/one.txt"));
    std::fs::remove_dir_all(&dir)?;
    assert_eq!(stored?, b"stored");
    Ok(())
}

#[actix_web::test]
async fn an_unauthenticated_upload_is_refused_whatever_its_size() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let state = state(&db.pool)?;
    let mut big = PNG.to_vec();
    big.resize(9 * 1024 * 1024, 0);
    for token in [None, Some("rok_unknown0_00000000000000000000000000000000")] {
        let answer = send(&state, token, Method::POST, MEDIA, &[], big.clone()).await?;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{token:?}");
        assert_eq!(answer.json()?["error"]["code"], "unauthorized");
    }
    db.drop().await
}
