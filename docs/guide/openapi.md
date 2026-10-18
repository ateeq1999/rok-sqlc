# The OpenAPI document

Every generated API describes itself: with `[api] output` set, `rok-sqlc g`
annotates each generated handler and type for
[utoipa](https://docs.rs/utoipa/5), and the scaffolded `app.rs` serves the
document at `/api-docs/openapi.json` with Swagger UI at `/docs/`. The
samples on this page come from [`docs/examples/api`](../examples/api).

`[api] openapi = false` turns all of it off. The generated files then
mention no utoipa, exactly as before the document existed, and you can
leave both dependencies out.

## What is generated

Each handler carries `#[utoipa::path]`: its method, its absolute path under
`[api] prefix`, a tag named after the route base (`tasks`), the handler's
name as its operation id, its params and body, `bearer_auth` when the
route needs a caller, and exactly the statuses the handler can answer.
Every error status has the wire standard's `ErrorBody`:

<!-- source: docs/examples/api/src/features/tasks/handlers.rs -->
```rust
#[utoipa::path(
    get,
    path = "/api/v1/tasks",
    tag = "tasks",
    operation_id = "list",
    params(ListTasksQuery),
    responses(
        (status = 200, description = "One page of rows.", body = PaginatedResponse<TaskResponse>),
        (status = 400, description = "A malformed path, query or body.", body = ErrorBody),
        (status = 401, description = "No valid bearer token.", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
```

The request and response bodies derive `ToSchema`, and the query strings
`IntoParams`; the data layer's typed ids, enums, sort keys and search
conditions derive `ToSchema` too. A typed id shows as its inner type (a
UUID is `{"type": "string", "format": "uuid"}`), and an enum by its labels.

`features/generated.rs` collects every generated path, the schemas utoipa
can't reach from them, one tag per feature, and the `bearer_auth` security
scheme (an HTTP bearer JWT, added by the `BearerAuth` modifier):

<!-- source: docs/examples/api/src/features/generated.rs -->
```rust
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        crate::shared::health::liveness,
        crate::shared::health::readiness,
        projects::handlers::list,
        projects::handlers::create,
        projects::handlers::get_one,
        projects::handlers::update,
        projects::handlers::remove,
        projects::handlers::list_tasks,
        tasks::handlers::list,
        tasks::handlers::create,
        tasks::handlers::get_one,
        tasks::handlers::update,
        tasks::handlers::remove,
    ),
```

`/health/live` and `/health/ready` are documented under the `health` tag,
as `liveness` and `readiness`.

## Your hand-written paths

`rok-sqlc init` scaffolds `src/openapi.rs`, which is yours. Its `ApiDoc`
lists your own handlers and schemas, and `document()` merges the generated
document into it, titled and versioned after your crate:

<!-- source: docs/examples/api/src/openapi.rs -->
```rust
/// Your hand-written paths and schemas: list a handler carrying
/// `#[utoipa::path]` in `paths(...)`, and a type deriving `ToSchema` that
/// no path refers to in `components(schemas(...))`. Where yours and the
/// generated document name the same path or schema, yours wins.
#[derive(OpenApi)]
#[openapi(paths(), components(schemas()))]
pub struct ApiDoc;

/// The whole document: [`ApiDoc`] with every generated path and schema
/// merged in, titled and versioned after the crate.
#[must_use]
pub fn document() -> utoipa::openapi::OpenApi {
    let mut doc = ApiDoc::openapi();
    doc.merge(GeneratedApiDoc::openapi());
    doc.info.title = env!("CARGO_PKG_NAME").to_owned();
    doc.info.version = env!("CARGO_PKG_VERSION").to_owned();
    doc
}
```

A hand-written handler documents itself the way the generated ones do:
annotate it with `#[utoipa::path(...)]`, give it a tag (a new one goes in
`ApiDoc`'s `tags(...)`), and add `security(("bearer_auth" = []))` when it
takes `AuthUser` or `AdminUser`. Then name it in `paths(...)`.

## Serving the document

The scaffolded `app.rs` mounts both routes. `/docs` redirects to `/docs/`,
where Swagger UI lives:

<!-- source: docs/examples/api/src/app.rs -->
```rust
pub fn configure(cfg: &mut web::ServiceConfig, pool: &PgPool, keys: &web::Data<JwtKeys>) {
    cfg.app_data(web::Data::new(pool.clone()))
        .app_data(keys.clone());
    register_services(cfg, pool);
    health::routes(cfg);
    cfg.service(web::redirect("/docs", "/docs/").see_other())
        .service(SwaggerUi::new("/docs/{_:.*}").url("/api-docs/openapi.json", document()));
    configure_all(cfg);
}
```

The dependencies are in `init`'s `rok-sqlc.toml` and the generated
`src/db/mod.rs`:

<!-- source: docs/examples/api/Cargo.toml -->
```toml
utoipa = { version = "5", features = ["chrono", "uuid"] }
utoipa-swagger-ui = { version = "9", features = ["actix-web", "vendored"] }
```

`vendored` bundles Swagger UI with the crate. Without it,
utoipa-swagger-ui's build script downloads Swagger UI, so every build
(and every CI run) would need the network.

## A project started before the document

`init` never rewrites a scaffolded file, so a project scaffolded before
rok-sqlc generated the document keeps its `lib.rs` and `app.rs`. After
`rok-sqlc g`, add by hand what `init` now scaffolds:

1. the `utoipa-swagger-ui` dependency above (`utoipa` is in the generated
   `src/db/mod.rs`'s list);
2. `src/openapi.rs`: run `rok-sqlc init` again, which writes only the
   files that are missing, or copy it from the sample above;
3. `pub mod openapi;` in `src/lib.rs`;
4. in `app.rs`, `use utoipa_swagger_ui::SwaggerUi;`,
   `use crate::openapi::document;` and the two `cfg.service(...)` lines of
   `configure`.
