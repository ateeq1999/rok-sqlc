# Testing an API

Test what is yours: the business rules in your services and the routes
whose behaviour you changed. The generated code (the data layer, the
handlers and the defaults) is tested by rok-sqlc. The guidelines' rule of
thumb: one service test per business rule, and one API test per route you
changed, the happy path plus one failure.

The samples come from the tests of [`docs/examples/api`](../examples/api)
(`tests/`). Both kinds run against PostgreSQL with
[`#[sqlx::test]`](https://docs.rs/sqlx/0.9/sqlx/attr.test.html), so they
need no mocks of the database.

## A database for each test

`#[sqlx::test]` gives each test a database of its own: it creates a fresh
one on the server `DATABASE_URL` names, applies every migration in
`migrations/`, passes the test a `PgPool` on it, and drops it after a test
that passes (a failing test's database is kept, to look at). Tests run in
parallel without seeing each other's rows.

It needs SQLx's `macros` and `migrate` features, which are on by default
(the generated code's `sqlx` dependency doesn't turn the defaults off), and
a `DATABASE_URL` whose user may create databases:

```sh
DATABASE_URL=postgres://postgres:postgres@localhost:5432/myapi cargo test
```

Tests live in `tests/`, outside `src/`. `check-arch` doesn't read them, so
they may use any crate. The clippy lints `init` suggests (`unwrap_used`,
`expect_used`, `panic`) apply to tests too: return a `Result` and use `?`,
as the example does.

## Shared helpers

The example's `tests/support/mod.rs` signs tokens for test callers with
the secret the test application verifies:

<!-- source: docs/examples/api/tests/support/mod.rs -->
```rust
/// What a test returns: `?` works on any error.
pub type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The secret the test application's `JwtKeys` use.
pub const SECRET: &str = "test secret";

/// A signed-in caller with a fresh id.
#[must_use]
pub fn caller() -> AuthUser {
    AuthUser {
        id: Uuid::now_v7(),
        role: Role::User,
    }
}

/// `user`'s `Authorization` header: a bearer token signed with [`SECRET`],
/// valid for an hour.
pub fn bearer(user: AuthUser) -> Result<String, Box<dyn std::error::Error>> {
    let exp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 3600;
    let claims = Claims {
        sub: user.id,
        role: user.role,
        exp,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(SECRET.as_bytes()),
    )?;
    Ok(format!("Bearer {token}"))
}
```

`jsonwebtoken`'s default header is HS256, which `shared/auth.rs` verifies.
An admin caller is the same with `Role::Admin`.

## Service tests

A service test calls the service as the handler would, with rows made
through the other services. It checks a business rule without HTTP. The
tasks service's rule is that a task goes only into one of the caller's
projects:

<!-- source: docs/examples/api/tests/service.rs -->
```rust
#[sqlx::test]
async fn a_task_goes_only_into_the_callers_project(pool: PgPool) -> TestResult {
    let projects = ProjectsService::new(pool.clone());
    let tasks = TasksService::new(pool.clone());
    let (alice, bob) = (caller(), caller());

    let project = projects
        .create(
            alice,
            CreateProjectDto {
                name: "Alice's".to_owned(),
                open_tasks: None,
                created_at: None,
            },
        )
        .await?;
    let result = tasks
        .create(
            bob,
            CreateTaskDto {
                project_id: project.id,
                title: "Not mine".to_owned(),
                done: None,
                created_at: None,
            },
        )
        .await;

    assert!(matches!(result, Err(AppError::NotFound(_))));
    let project = projects.get_one(alice, project.id).await?;
    assert_eq!(project.open_tasks, 0);
    Ok(())
}
```

The service methods come from the generated trait, so bring it into scope
(`use …::tasks::{TasksService, TasksServiceApi};`). Here the port is the
real one, which `TasksService::new` builds over the same pool. To test a
service without the feature behind its port, give the service a second
constructor taking the port (`with_projects(pool, projects: Arc<dyn
ProjectTasks>)`), keep `new(pool)` for `register_services`, and pass a fake
in the test.

## API tests

An API test builds the whole application with `app::configure`, the
function `main.rs` serves, and calls it with `actix_web::test`. No server
starts and no port opens:

<!-- source: docs/examples/api/tests/api.rs -->
```rust
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
```

A failure checks the status and the wire standard's code, which is what
clients branch on:

<!-- source: docs/examples/api/tests/api.rs -->
```rust
    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body: Value = test::read_body_json(response).await;
    assert_eq!(body["error"]["code"], "not_found");
```

The example's API tests also cover a missing token (`401`), a body over
its length rule (`422` with `details`), and the hand-written `stats` route
counting a task created through the API.

Under axum, an API test builds the same router `main.rs` serves,
`app::router(AppState::new(..))`, and sends it a request with
`tower::ServiceExt::oneshot` (`tower` with its `util` feature, as a
dev-dependency). No server starts here either. The axum template's
`tests/support/mod.rs` wraps this as `call`:

<!-- source: templates/axum-api/tests/support/mod.rs -->
```rust
    let response = router(state.clone()).oneshot(request).await?;
```

A test without the peer's address (`ConnectInfo`) counts against the
`/auth` rate limit as `0.0.0.0`.

## In CI

Run the tests against a PostgreSQL service with `DATABASE_URL` set, beside
the other checks ([Deploying](deploying.md#checks-in-ci)). Use the major
version you run in production: rok-sqlc's own CI runs PostgreSQL 17.
