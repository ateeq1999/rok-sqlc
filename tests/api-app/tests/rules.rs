//! String rules (`[api.rules]`): `/api/v1/profiles` has a rule or
//! transform on each column. Transforms run while the body is read, so the
//! row is stored transformed; each failed rule is a `422` under its field,
//! with the rule's VineJS name as the code.

mod common;

use actix_web::http::{Method, StatusCode};
use common::{Database, call};
use serde_json::{Value, json};

const PROFILES: &str = "/api/v1/profiles";

/// A body every rule accepts.
fn valid() -> Value {
    json!({
        "email": "  Ada@Example.COM ",
        "shout": "hi",
        "contact": "A.B+news@GoogleMail.com",
        "website": "https://example.com/about",
        "ref_uuid": "0b0d5b2b-6c47-4c7e-9f0e-3c7a4b1a2d3e",
        "ref_ulid": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "ip": "192.168.0.1",
        "phone": "+966512345678",
        "first_name": "Ada Lovelace",
        "handle": "ada-99",
        "plain": "plain text",
        "slug": "ada-99",
        "country": "SA",
        "username": "ada",
        "sku": "SKU-1",
        "file_name": "cv.pdf",
        "bio": "hello",
        "pin": "1234",
        "color": "#ff00aa",
        "iban": "SA03 8000 0000 6080 1016 7519",
        "token": "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2ln",
        "zip": "12345",
        "card": "4111 1111 1111 1111",
        "location": "(24.7136, 46.6753)",
    })
}

#[actix_web::test]
async fn transforms_are_stored_transformed() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let created = call(&db, Method::POST, PROFILES, Some(valid())).await;
    assert_eq!(created.status, StatusCode::CREATED, "{created:?}");
    assert_eq!(created.body["email"], "ada@example.com");
    assert_eq!(created.body["shout"], "HI");
    assert_eq!(created.body["contact"], "ab@gmail.com");
    let stored: (String, String, String) =
        sqlx::query_as("SELECT email, shout, contact FROM profiles")
            .fetch_one(&db.pool)
            .await
            .expect("the stored profile");
    assert_eq!(
        stored,
        (
            "ada@example.com".into(),
            "HI".into(),
            "ab@gmail.com".into()
        )
    );

    // The update body transforms too, and `null` still clears a column.
    let one = format!("{PROFILES}/{}", created.body["id"].as_str().expect("an id"));
    let updated = call(
        &db,
        Method::PATCH,
        &one,
        Some(json!({"email": " Grace@Example.com", "shout": "hey", "contact": null})),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK, "{updated:?}");
    let stored: (String, String, Option<String>) =
        sqlx::query_as("SELECT email, shout, contact FROM profiles")
            .fetch_one(&db.pool)
            .await
            .expect("the stored profile");
    assert_eq!(stored, ("grace@example.com".into(), "HEY".into(), None));
    db.drop().await;
}

#[actix_web::test]
async fn each_rule_rejects_with_its_code() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let cases = [
        ("email", "nope", "email"),
        ("website", "not a url", "url"),
        ("ref_uuid", "0b0d5b2b-6c47-7c7e-9f0e-3c7a4b1a2d3e", "uuid"),
        ("ref_ulid", "81ARZ3NDEKTSV4RRFFQ69G5FAV", "ulid"),
        ("ip", "::1", "ipAddress"),
        ("phone", "0123", "mobile"),
        ("first_name", "Ada_1", "alpha"),
        ("handle", "ada 99", "alphaNumeric"),
        ("plain", "café", "ascii"),
        ("slug", "Ada!", "regex"),
        ("country", "US", "in"),
        ("username", "root", "notIn"),
        ("sku", "X-1", "startsWith"),
        ("file_name", "cv.doc", "endsWith"),
        ("bio", "hi", "minLength"),
        ("bio", "hello world!", "maxLength"),
        ("pin", "123", "fixedLength"),
        ("color", "ff00aa", "hexCode"),
        ("iban", "SA0380000000608010167518", "iban"),
        ("token", "a.b", "jwt"),
        ("zip", "1234", "postalCode"),
        ("card", "4111 1111 1111 1112", "creditCard"),
        ("location", "91,0", "coordinates"),
    ];
    for (field, value, code) in cases {
        let mut body = valid();
        body[field] = json!(value);
        let answer = call(&db, Method::POST, PROFILES, Some(body)).await;
        assert_eq!(
            answer.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{field} = {value}: {answer:?}"
        );
        assert_eq!(answer.code(), "validation_failed", "{answer:?}");
        let details = answer.body["error"]["details"]
            .as_object()
            .expect("the details");
        assert_eq!(details.len(), 1, "{field} = {value}: {answer:?}");
        let detail = &details[field][0];
        assert_eq!(detail["code"], code, "{field} = {value}: {answer:?}");
        let message = detail["message"].as_str().expect("a message");
        assert!(message.starts_with(&format!("{field} must ")), "{message}");
    }
    // The update body checks the same rules.
    let created = call(&db, Method::POST, PROFILES, Some(valid())).await;
    assert_eq!(created.status, StatusCode::CREATED, "{created:?}");
    let one = format!("{PROFILES}/{}", created.body["id"].as_str().expect("an id"));
    let answer = call(&db, Method::PATCH, &one, Some(json!({"country": "US"}))).await;
    assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY, "{answer:?}");
    assert_eq!(answer.body["error"]["details"]["country"][0]["code"], "in");
    db.drop().await;
}

/// `url` checks as VineJS's default does (validator.js's `isURL`): the
/// scheme may be left out, but must be `http`, `https` or `ftp` when given,
/// and the host needs a top-level domain unless it is an IP address.
#[actix_web::test]
async fn url_checks_as_vinejs_does() {
    let Some(db) = Database::fresh().await else {
        return;
    };
    let accepted = [
        "example.com",
        "www.example.com/a/b?q=1#top",
        "https://example.com:8080/path",
        "ftp://user:secret@files.example.org",
        "http://192.168.0.1/admin",
        "http://[::1]:3000",
        "https://مثال.السعودية",
    ];
    for url in accepted {
        let mut body = valid();
        body["website"] = json!(url);
        let answer = call(&db, Method::POST, PROFILES, Some(body)).await;
        assert_eq!(answer.status, StatusCode::CREATED, "{url}: {answer:?}");
    }
    let rejected = [
        "http://localhost",
        "mailto:ada@example.com",
        "javascript:alert(1)",
        "file:///etc/passwd",
        "https://exa_mple.com",
        "https://-example.com",
        "https://example.com:0",
        "https://example.com:65536",
        "//example.com",
        "https://example.123",
        "https://example .com",
        "https://",
    ];
    for url in rejected {
        let mut body = valid();
        body["website"] = json!(url);
        let answer = call(&db, Method::POST, PROFILES, Some(body)).await;
        assert_eq!(
            answer.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{url}: {answer:?}"
        );
        assert_eq!(answer.body["error"]["details"]["website"][0]["code"], "url");
    }
    db.drop().await;
}
