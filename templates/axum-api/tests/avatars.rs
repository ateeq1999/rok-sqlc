//! Profile photos (`/users/me/avatar`): an uploaded image becomes a
//! 256×256 JPEG stored through the `Storage` port, answered as every user
//! response's `avatar_url`, and served publicly from `/avatars/{file}`.

mod support;

use std::io::Cursor;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::{Method, StatusCode};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use serde_json::Value;
use support::{Database, Raw, Recorded, TestResult, admin, send, state, user};

type AppState = axum_api::app::AppState;

const AVATAR: &str = "/api/v1/users/me/avatar";

/// A 400×300 image encoded as `format`.
fn picture(format: ImageFormat) -> Result<Vec<u8>, image::ImageError> {
    let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(400, 300, Rgba([30, 90, 200, 255])));
    let image = if format == ImageFormat::Jpeg {
        DynamicImage::ImageRgb8(image.to_rgb8())
    } else {
        image
    };
    let mut bytes = Vec::new();
    image.write_to(&mut Cursor::new(&mut bytes), format)?;
    Ok(bytes)
}

/// The CRC-32 a PNG chunk ends with.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Whether `haystack` holds `needle`.
fn holds(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// `PUT /users/me/avatar` of `bytes`.
async fn upload(
    state: &AppState,
    token: &str,
    bytes: Vec<u8>,
) -> Result<Raw, Box<dyn std::error::Error>> {
    let headers = [("content-type", "application/octet-stream")];
    send(state, Some(token), Method::PUT, AVATAR, &headers, bytes).await
}

/// `method uri`, with `token` when given and no body.
async fn fetch(
    state: &AppState,
    token: Option<&str>,
    method: Method,
    uri: &str,
) -> Result<Raw, Box<dyn std::error::Error>> {
    send(state, token, method, uri, &[], Vec::new()).await
}

/// The `avatar_url` of `answer`'s user, checked to name a generated file.
fn avatar_url(answer: &Raw) -> Result<String, Box<dyn std::error::Error>> {
    let body = answer.json()?;
    let url = body["avatar_url"].as_str().ok_or("no avatar_url")?;
    let file = url.strip_prefix("/api/v1/avatars/").ok_or(url.to_owned())?;
    let name = file.strip_suffix(".jpg").ok_or(url.to_owned())?;
    assert_eq!(name.len(), 32, "{url}");
    assert!(
        name.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{url}"
    );
    Ok(url.to_owned())
}

/// The storage key the file of `url` is kept under.
fn key_of(url: &str) -> String {
    url.replacen("/api/v1/", "", 1)
}

#[tokio::test]
async fn each_format_is_stored_as_a_square_jpeg_and_served_to_anyone() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let recorded = Recorded::new();
    let state = state(&db.pool)?.with_storage(Arc::new(recorded.clone()));
    let token = user(&db.pool).await?;
    let admin = admin(&db.pool).await?;
    for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
        let answer = upload(&state, &token, picture(format)?).await?;
        assert_eq!(
            answer.status,
            StatusCode::OK,
            "{format:?}: {:?}",
            answer.json()
        );
        let url = avatar_url(&answer)?;
        assert_eq!(recorded.keys(), vec![key_of(&url)], "{format:?}");

        let served = fetch(&state, None, Method::GET, &url).await?;
        assert_eq!(served.status, StatusCode::OK);
        assert_eq!(served.header("content-type"), Some("image/jpeg"));
        assert_eq!(
            served.header("cache-control"),
            Some("public, max-age=31536000, immutable")
        );
        let decoded = image::load_from_memory_with_format(&served.bytes, ImageFormat::Jpeg)?;
        assert_eq!((decoded.width(), decoded.height()), (256, 256));

        // Every user response answers it: the account's and the admin's.
        let me = fetch(&state, Some(&token), Method::GET, "/api/v1/users/me").await?;
        assert_eq!(me.json()?["avatar_url"], url.as_str());
        let id = me.json()?["id"].as_str().ok_or("no id")?.to_owned();
        let row = fetch(
            &state,
            Some(&admin),
            Method::GET,
            &format!("/api/v1/users/{id}"),
        )
        .await?;
        assert_eq!(row.json()?["avatar_url"], url.as_str());
    }
    db.drop().await
}

#[tokio::test]
async fn a_new_photo_replaces_the_old_one() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let recorded = Recorded::new();
    let state = state(&db.pool)?.with_storage(Arc::new(recorded.clone()));
    let token = user(&db.pool).await?;
    let first = avatar_url(&upload(&state, &token, picture(ImageFormat::Png)?).await?)?;
    let second = avatar_url(&upload(&state, &token, picture(ImageFormat::Png)?).await?)?;
    assert_ne!(first, second);
    assert_eq!(recorded.keys(), vec![key_of(&second)]);
    let gone = fetch(&state, None, Method::GET, &first).await?;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
    db.drop().await
}

#[tokio::test]
async fn delete_clears_the_photo() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let recorded = Recorded::new();
    let state = state(&db.pool)?.with_storage(Arc::new(recorded.clone()));
    let token = user(&db.pool).await?;
    let url = avatar_url(&upload(&state, &token, picture(ImageFormat::Png)?).await?)?;

    let answer = fetch(&state, Some(&token), Method::DELETE, AVATAR).await?;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.json()?["avatar_url"], Value::Null);
    assert!(recorded.keys().is_empty());
    let gone = fetch(&state, None, Method::GET, &url).await?;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);

    // Without a photo, it is still a `200`.
    let again = fetch(&state, Some(&token), Method::DELETE, AVATAR).await?;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(again.json()?["avatar_url"], Value::Null);
    db.drop().await
}

#[tokio::test]
async fn what_is_not_an_image_is_unprocessable() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let recorded = Recorded::new();
    let state = state(&db.pool)?.with_storage(Arc::new(recorded.clone()));
    let token = user(&db.pool).await?;
    for garbage in [
        &b"not an image at all"[..],
        b"",
        b"\x89PNG\r\n\x1a\n then garbage",
    ] {
        let answer = upload(&state, &token, garbage.to_vec()).await?;
        assert_eq!(
            answer.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{garbage:?}"
        );
        assert_eq!(answer.json()?["error"]["code"], "validation_failed");
    }
    assert!(recorded.keys().is_empty());
    let me = fetch(&state, Some(&token), Method::GET, "/api/v1/users/me").await?;
    assert_eq!(me.json()?["avatar_url"], Value::Null);
    db.drop().await
}

#[tokio::test]
async fn a_body_over_2_mib_is_too_large() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let state = state(&db.pool)?;
    let token = user(&db.pool).await?;
    let mut big = picture(ImageFormat::Png)?;
    big.resize(2 * 1024 * 1024 + 1, 0);
    let answer = upload(&state, &token, big).await?;
    assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(answer.json()?["error"]["code"], "payload_too_large");
    db.drop().await
}

#[tokio::test]
async fn a_decompression_bomb_is_refused_quickly() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let recorded = Recorded::new();
    let state = state(&db.pool)?.with_storage(Arc::new(recorded.clone()));
    let token = user(&db.pool).await?;
    // A real PNG whose header claims 100000×100000 pixels.
    let mut bomb = picture(ImageFormat::Png)?;
    bomb[16..20].copy_from_slice(&100_000_u32.to_be_bytes());
    bomb[20..24].copy_from_slice(&100_000_u32.to_be_bytes());
    let crc = crc32(&bomb[12..29]);
    bomb[29..33].copy_from_slice(&crc.to_be_bytes());

    let started = Instant::now();
    let answer = upload(&state, &token, bomb).await?;
    assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(recorded.keys().is_empty());
    db.drop().await
}

#[tokio::test]
async fn exif_never_reaches_the_served_file() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let state = state(&db.pool)?;
    let token = user(&db.pool).await?;
    let jpeg = picture(ImageFormat::Jpeg)?;
    let payload = b"Exif\0\0II*\0\x08\0\0\0GPS 51.5N 0.1W secret";
    let length = u16::try_from(payload.len() + 2)?.to_be_bytes();
    let mut with_exif = jpeg[..2].to_vec();
    with_exif.extend_from_slice(&[0xFF, 0xE1, length[0], length[1]]);
    with_exif.extend_from_slice(payload);
    with_exif.extend_from_slice(&jpeg[2..]);

    let url = avatar_url(&upload(&state, &token, with_exif).await?)?;
    let served = fetch(&state, None, Method::GET, &url).await?;
    assert_eq!(served.status, StatusCode::OK);
    assert!(!holds(&served.bytes, b"Exif"));
    assert!(!holds(&served.bytes, b"secret"));
    db.drop().await
}

#[tokio::test]
async fn a_file_name_that_was_never_generated_is_not_found() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let state = state(&db.pool)?;
    for file in [
        "0123456789abcdef0123456789abcdef.jpg",
        "0123456789abcdef0123456789abcdef.png",
        "0123456789ABCDEF0123456789abcdef.jpg",
        "secret.jpg",
        "..%2Fmedia%2F0123456789abcdef0123456789abcdef",
        "%2E%2E",
    ] {
        let answer = fetch(
            &state,
            None,
            Method::GET,
            &format!("/api/v1/avatars/{file}"),
        )
        .await?;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{file}");
    }
    db.drop().await
}

#[tokio::test]
async fn changing_the_photo_needs_a_token() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let state = state(&db.pool)?;
    let headers = [("content-type", "application/octet-stream")];
    let bytes = picture(ImageFormat::Png)?;
    let answer = send(&state, None, Method::PUT, AVATAR, &headers, bytes).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    let answer = fetch(&state, None, Method::DELETE, AVATAR).await?;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    db.drop().await
}
