//! The actix-web extractor generated with `extractors = ["axum",
//! "actix_web"]` (`src/validated_db/http/`), driven through a real `App`
//! with `actix_web::test`, and checked against the axum extractor generated
//! beside it. No database needed.

use actix_web::http::StatusCode;
use actix_web::http::header::CONTENT_TYPE;
use actix_web::{App, test, web};
use rok_sqlc_fixture_app::validated_db::http::actix_web::ValidatedJson;
use rok_sqlc_fixture_app::validated_db::teams::models::NewTeam;
use serde::Deserialize;
use serde_json::{Value, json};
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

fn describe_team(team: NewTeam) -> String {
    format!("{} {}", team.code.0, team.name)
}

fn describe_signup(signup: &Signup) -> String {
    let plan = match signup.plan {
        Plan::Free => "free",
        Plan::Pro => "pro",
    };
    format!("{} {plan}", signup.email)
}

async fn create_team(team: ValidatedJson<NewTeam>) -> String {
    // `Deref` reaches the body; `into_inner` takes it.
    assert!(!team.name.is_empty());
    describe_team(team.into_inner())
}

async fn sign_up(ValidatedJson(signup): ValidatedJson<Signup>) -> String {
    describe_signup(&signup)
}

/// actix-web's default `PayloadConfig` limit.
const DEFAULT_LIMIT: usize = 262_144;

/// A response as the tests compare it: status, `Content-Type`, body.
type Answer = (u16, Option<String>, String);

/// Posts `body` to `path` on the actix-web app with `content_type`, if any,
/// and a `PayloadConfig` limit of `limit` bytes.
async fn actix_answer(path: &str, content_type: Option<&str>, body: &str, limit: usize) -> Answer {
    let app = test::init_service(
        App::new()
            .app_data(web::PayloadConfig::new(limit))
            .route("/teams", web::post().to(create_team))
            .route("/signups", web::post().to(sign_up)),
    )
    .await;
    let mut request = test::TestRequest::post().uri(path);
    if let Some(content_type) = content_type {
        request = request.insert_header((CONTENT_TYPE, content_type));
    }
    let request = request.set_payload(body.to_owned()).to_request();
    let response = test::call_service(&app, request).await;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .map(|value| value.to_str().unwrap().to_owned());
    let bytes = test::read_body(response).await;
    (
        status,
        content_type,
        String::from_utf8(bytes.to_vec()).unwrap(),
    )
}

/// Posts `body` to `path` on an axum app with the same routes, through the
/// axum extractor generated beside the actix-web one.
async fn axum_answer(path: &str, content_type: Option<&str>, body: &str) -> Answer {
    use axum::body::{Body, to_bytes};
    use axum::routing::post;
    use rok_sqlc_fixture_app::validated_db::http::axum::ValidatedJson;
    use tower::ServiceExt;

    let app = axum::Router::new()
        .route(
            "/teams",
            post(|ValidatedJson(team): ValidatedJson<NewTeam>| async move {
                describe_team(team)
            }),
        )
        .route(
            "/signups",
            post(|ValidatedJson(signup): ValidatedJson<Signup>| async move {
                describe_signup(&signup)
            }),
        );
    let mut request = axum::http::Request::post(path);
    if let Some(content_type) = content_type {
        request = request.header(axum::http::header::CONTENT_TYPE, content_type);
    }
    let request = request.body(Body::from(body.to_owned())).unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .map(|value| value.to_str().unwrap().to_owned());
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        content_type,
        String::from_utf8(bytes.to_vec()).unwrap(),
    )
}

/// Posts `body` to `path` with `content_type`, if any; returns the status
/// and the body as text.
async fn post_raw(path: &str, content_type: Option<&str>, body: &str) -> (StatusCode, String) {
    let (status, _, text) = actix_answer(path, content_type, body, DEFAULT_LIMIT).await;
    (StatusCode::from_u16(status).unwrap(), text)
}

/// Posts JSON `body` to `path`; returns the status and the body as JSON.
async fn post_json(path: &str, body: &str) -> (StatusCode, Value) {
    let (status, text) = post_raw(path, Some("application/json"), body).await;
    (status, serde_json::from_str(&text).unwrap())
}

fn envelope(message: &str, code: &str) -> Value {
    json!({"success": false, "error": {"message": message, "code": code}})
}

#[actix_web::test]
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

#[actix_web::test]
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

#[actix_web::test]
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

#[actix_web::test]
async fn a_body_over_the_payload_limit_is_a_malformed_body() {
    let body = r#"{"code": "NORTH", "name": "Northern"}"#;
    let (status, content_type, text) =
        actix_answer("/teams", Some("application/json"), body, 8).await;
    assert_eq!(status, 400);
    assert_eq!(content_type.as_deref(), Some("application/json"));
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap(),
        envelope("payload reached size limit", "MALFORMED_BODY")
    );
}

#[actix_web::test]
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

#[actix_web::test]
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

#[actix_web::test]
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

/// The same request gets the same status, `Content-Type` and body from both
/// frameworks, accepted or rejected. A JSON body is compared as JSON:
/// validator keeps an error's `params` in a `HashMap`, whose key order
/// differs from one rejection to the next whichever framework answers.
#[actix_web::test]
async fn both_frameworks_answer_the_same_input_with_the_same_body() {
    fn comparable((status, content_type, text): Answer) -> (u16, Option<String>, Value) {
        let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (status, content_type, body)
    }

    let team = r#"{"code": "NORTH", "name": "Northern"}"#;
    let json = Some("application/json");
    let cases = [
        ("/teams", json, team),
        ("/teams", Some("application/merge-patch+json"), team),
        ("/teams", json, r#"{"code": "NORTH","#),
        ("/teams", json, ""),
        ("/teams", None, team),
        ("/teams", Some("text/plain"), team),
        ("/teams", json, r#"{"code": "NORTH"}"#),
        ("/teams", json, r#"{"code": "NORTH", "name": 7}"#),
        ("/teams", json, r#"{"code": "NORTH", "name": "N"}"#),
        (
            "/signups",
            json,
            r#"{"email": "ada@example.com", "plan": "pro"}"#,
        ),
        ("/signups", json, r#"{"email": "ada", "plan": "free"}"#),
        (
            "/signups",
            json,
            r#"{"email": "ada@example.com", "plan": "gold"}"#,
        ),
    ];
    for (path, content_type, body) in cases {
        let actix = comparable(actix_answer(path, content_type, body, DEFAULT_LIMIT).await);
        let axum = comparable(axum_answer(path, content_type, body).await);
        assert_eq!(actix, axum, "{path} {content_type:?} {body}");
    }
}
