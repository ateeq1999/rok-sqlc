//! API tests: the whole application, built by `app::configure` as `main.rs`
//! builds it, over a database `#[sqlx::test]` creates and migrates for each
//! test. Run them with `DATABASE_URL` set to a server the tests may create
//! databases on.

mod support;

use actix_web::http::{StatusCode, header};
use actix_web::{App, test, web};
use rok_sqlc_api_example::app::configure;
use rok_sqlc_api_example::shared::auth::JwtKeys;
use serde_json::{Value, json};
use sqlx::PgPool;

use support::{SECRET, TestResult, bearer, caller};

#[sqlx::test]
async fn a_created_project_is_at_its_location(pool: PgPool) -> TestResult {
    let keys = web::Data::new(JwtKeys::new(SECRET));
    let app = test::init_service(App::new().configure(|cfg| configure(cfg, &pool, &keys))).await;
    let alice = bearer(caller())?;

    let request = test::TestRequest::post()
        .uri("/api/v1/projects")
        .insert_header((header::AUTHORIZATION, alice.as_str()))
        .set_json(json!({"name": "Launch"}))
        .to_request();
    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let location = response
        .headers()
        .get(header::LOCATION)
        .ok_or("no Location")?
        .to_str()?
        .to_owned();

    let request = test::TestRequest::get()
        .uri(&location)
        .insert_header((header::AUTHORIZATION, alice.as_str()))
        .to_request();
    let project: Value = test::call_and_read_body_json(&app, request).await;
    assert_eq!(project["name"], "Launch");
    assert_eq!(project["open_tasks"], 0);
    Ok(())
}

#[sqlx::test]
async fn another_callers_project_is_not_found(pool: PgPool) -> TestResult {
    let keys = web::Data::new(JwtKeys::new(SECRET));
    let app = test::init_service(App::new().configure(|cfg| configure(cfg, &pool, &keys))).await;

    let request = test::TestRequest::post()
        .uri("/api/v1/projects")
        .insert_header((header::AUTHORIZATION, bearer(caller())?))
        .set_json(json!({"name": "Alice's"}))
        .to_request();
    let project: Value = test::call_and_read_body_json(&app, request).await;
    let id = project["id"].as_str().ok_or("no id")?;

    let request = test::TestRequest::get()
        .uri(&format!("/api/v1/projects/{id}"))
        .insert_header((header::AUTHORIZATION, bearer(caller())?))
        .to_request();
    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body: Value = test::read_body_json(response).await;
    assert_eq!(body["error"]["code"], "not_found");
    Ok(())
}

#[sqlx::test]
async fn a_request_without_a_token_is_unauthorized(pool: PgPool) -> TestResult {
    let keys = web::Data::new(JwtKeys::new(SECRET));
    let app = test::init_service(App::new().configure(|cfg| configure(cfg, &pool, &keys))).await;

    let request = test::TestRequest::get()
        .uri("/api/v1/projects")
        .to_request();
    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

#[sqlx::test]
async fn a_task_counts_on_its_project_and_in_the_stats(pool: PgPool) -> TestResult {
    let keys = web::Data::new(JwtKeys::new(SECRET));
    let app = test::init_service(App::new().configure(|cfg| configure(cfg, &pool, &keys))).await;
    let alice = bearer(caller())?;

    let request = test::TestRequest::post()
        .uri("/api/v1/projects")
        .insert_header((header::AUTHORIZATION, alice.as_str()))
        .set_json(json!({"name": "Launch"}))
        .to_request();
    let project: Value = test::call_and_read_body_json(&app, request).await;
    let request = test::TestRequest::post()
        .uri("/api/v1/tasks")
        .insert_header((header::AUTHORIZATION, alice.as_str()))
        .set_json(json!({"project_id": project["id"], "title": "Write the guide"}))
        .to_request();
    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::CREATED);

    let request = test::TestRequest::get()
        .uri("/api/v1/stats")
        .insert_header((header::AUTHORIZATION, alice.as_str()))
        .to_request();
    let stats: Value = test::call_and_read_body_json(&app, request).await;
    assert_eq!(stats, json!({"projects": 1, "open_tasks": 1}));
    Ok(())
}

#[sqlx::test]
async fn a_name_over_its_length_fails_validation(pool: PgPool) -> TestResult {
    let keys = web::Data::new(JwtKeys::new(SECRET));
    let app = test::init_service(App::new().configure(|cfg| configure(cfg, &pool, &keys))).await;

    let request = test::TestRequest::post()
        .uri("/api/v1/projects")
        .insert_header((header::AUTHORIZATION, bearer(caller())?))
        .set_json(json!({"name": "x".repeat(81)}))
        .to_request();
    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: Value = test::read_body_json(response).await;
    assert_eq!(body["error"]["code"], "validation_failed");
    assert_eq!(body["error"]["details"]["name"][0]["code"], "length");
    Ok(())
}
