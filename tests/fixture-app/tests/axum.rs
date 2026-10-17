//! The axum extractor generated with `extractors = ["axum"]`
//! (`src/validated_db/http/`), driven through a real `Router`. No database
//! needed.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::DefaultBodyLimit;
use axum::http::{Request, StatusCode, header};
use axum::routing::post;
use rok_sqlc_fixture_app::validated_db::http::axum::ValidatedJson;
use rok_sqlc_fixture_app::validated_db::teams::models::NewTeam;
use serde::Deserialize;
use serde_json::{Value, json};
use tower::ServiceExt;
use validator::Validate;

/// A hand-written request type, as an app writes one.
#[derive(Debug, Deserialize, Validate)]
struct Signup {
    #[validate(email(message = "email must be an email address"))]
    email: String,
    plan: Plan,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Plan {
    Free,
    Pro,
}

fn app() -> Router {
    Router::new()
        .route(
            "/teams",
            post(|team: ValidatedJson<NewTeam>| async move {
                let code = team.code.0.clone();
                format!("{code} {}", team.into_inner().name)
            }),
        )
        .route(
            "/signups",
            post(|ValidatedJson(signup): ValidatedJson<Signup>| async move {
                let plan = match signup.plan {
                    Plan::Free => "free",
                    Plan::Pro => "pro",
                };
                format!("{} {plan}", signup.email)
            }),
        )
}

/// Posts `body` to `path` with `content_type`, if any; returns the status
/// and the body as text.
async fn post_raw(path: &str, content_type: Option<&str>, body: &str) -> (StatusCode, String) {
    let mut request = Request::post(path);
    if let Some(content_type) = content_type {
        request = request.header(header::CONTENT_TYPE, content_type);
    }
    let request = request.body(Body::from(body.to_owned())).unwrap();
    let response = app().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

/// Posts JSON `body` to `path`; returns the status and the body as JSON.
async fn post_json(path: &str, body: &str) -> (StatusCode, Value) {
    let (status, text) = post_raw(path, Some("application/json"), body).await;
    (status, serde_json::from_str(&text).unwrap())
}

fn envelope(message: &str, code: &str) -> Value {
    json!({"success": false, "error": {"message": message, "code": code}})
}

#[tokio::test]
async fn a_valid_body_reaches_the_handler() {
    let body = r#"{"code": "NORTH", "name": "Northern"}"#;
    assert_eq!(
        post_raw("/teams", Some("application/json"), body).await,
        (StatusCode::OK, "NORTH Northern".to_owned())
    );
    // Parameters and `+json` types are JSON too.
    assert_eq!(
        post_raw("/teams", Some("application/json; charset=utf-8"), body)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        post_raw("/teams", Some("application/merge-patch+json"), body)
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn bad_syntax_is_a_malformed_body() {
    assert_eq!(
        post_json("/teams", r#"{"code": "NORTH","#).await,
        (
            StatusCode::BAD_REQUEST,
            envelope(
                "the request body is not valid JSON: EOF while parsing a value at line 1 column 17",
                "MALFORMED_BODY"
            )
        )
    );
    assert_eq!(
        post_json("/teams", "").await,
        (
            StatusCode::BAD_REQUEST,
            envelope(
                "the request body is not valid JSON: EOF while parsing a value at line 1 column 0",
                "MALFORMED_BODY"
            )
        )
    );
}

#[tokio::test]
async fn a_missing_or_wrong_content_type_is_a_malformed_body() {
    let body = r#"{"code": "NORTH", "name": "Northern"}"#;
    let expected = envelope(
        "expected a request with `Content-Type: application/json`",
        "MALFORMED_BODY",
    );
    for content_type in [None, Some("text/plain"), Some("application/jsonp")] {
        let (status, text) = post_raw("/teams", content_type, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{content_type:?}");
        assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap(),
            expected,
            "{content_type:?}"
        );
    }
}

/// The extractor reads the body through `axum::Json`, so the router's
/// `DefaultBodyLimit` applies, and a body over it is answered `413`.
#[tokio::test]
async fn a_body_over_the_default_body_limit_is_too_large() {
    let body = r#"{"code": "NORTH", "name": "Northern"}"#;
    let send = |limit: usize| async move {
        let request = Request::post("/teams")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
        let response = app()
            .layer(DefaultBodyLimit::max(limit))
            .oneshot(request)
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    };
    let (status, text) = send(8).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap(),
        envelope(
            "the request body is over the size limit",
            "PAYLOAD_TOO_LARGE"
        )
    );
    assert_eq!(
        send(body.len()).await,
        (StatusCode::OK, "NORTH Northern".to_owned())
    );
}

#[tokio::test]
async fn json_that_does_not_fit_the_type_fails_validation_without_details() {
    assert_eq!(
        post_json("/teams", r#"{"code": "NORTH"}"#).await,
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            envelope(
                "missing field `name` at line 1 column 17",
                "VALIDATION_FAILED"
            )
        )
    );
    assert_eq!(
        post_json("/teams", r#"{"code": "NORTH", "name": 7}"#).await,
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            envelope(
                "invalid type: integer `7`, expected a string at line 1 column 27",
                "VALIDATION_FAILED"
            )
        )
    );
}

#[tokio::test]
async fn a_body_breaking_rules_fails_validation_with_details() {
    let (status, body) = post_json("/teams", r#"{"code": "NORTH", "name": "N"}"#).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body,
        json!({
            "success": false,
            "error": {
                "message": "the request body failed validation",
                "code": "VALIDATION_FAILED",
                "details": {
                    "name": [{
                        "code": "length",
                        "message": "name must be between 2 and 40 characters",
                        "params": {"min": 2, "max": 40, "value": "N"}
                    }]
                }
            }
        })
    );
}

#[tokio::test]
async fn a_hand_written_type_goes_through_the_same_extractor() {
    assert_eq!(
        post_raw(
            "/signups",
            Some("application/json"),
            r#"{"email": "ada@example.com", "plan": "pro"}"#
        )
        .await,
        (StatusCode::OK, "ada@example.com pro".to_owned())
    );
    assert_eq!(
        post_json("/signups", r#"{"email": "ada", "plan": "free"}"#).await,
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({
                "success": false,
                "error": {
                    "message": "the request body failed validation",
                    "code": "VALIDATION_FAILED",
                    "details": {
                        "email": [{
                            "code": "email",
                            "message": "email must be an email address",
                            "params": {"value": "ada"}
                        }]
                    }
                }
            })
        )
    );
    // An unknown label is a data error: no details.
    assert_eq!(
        post_json(
            "/signups",
            r#"{"email": "ada@example.com", "plan": "gold"}"#
        )
        .await,
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            envelope(
                "unknown variant `gold`, expected `free` or `pro` at line 1 column 43",
                "VALIDATION_FAILED"
            )
        )
    );
}
