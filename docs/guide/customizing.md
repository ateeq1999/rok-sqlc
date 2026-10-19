# Customizing the generated API

The generated features cover CRUD, lists, relationships and the wire
standard. Everything else is yours: business rules, extra state, writes
across features, and features with no table. This page shows where each
goes, with samples from [`docs/examples/api`](../examples/api). The rules
behind it are in the [architecture guidelines](architecture.md).

## Overriding a service method

Each feature's `service_api.rs` declares a trait, `TasksServiceApi`, with
one method per endpoint and a working default for each. The feature's
`service.rs` implements it and is yours: `rok-sqlc g` writes it once and
never again. A business rule is an override there; every method you don't
override keeps its default, so an endpoint a later schema change adds
arrives working.

Every write comes in two methods:

- `create_in(&self, conn: &mut PgConnection, …)` holds the logic, on a
  connection or transaction the caller owns;
- `create(&self, …)` opens a transaction, calls `create_in`, and commits.

Override the `_in` method. The handler calls `create`, which still wraps
your logic in a transaction, and a port in another feature can call your
`create_in` inside its own transaction. Override the wrapper only to change
the transaction itself. Reads (`list`, `get_one`, lookups) have no `_in`
part: override them directly.

The example's tasks service overrides `create_in`: a task goes only into
one of the caller's projects, and counts as one of its open tasks.

<!-- source: docs/examples/api/src/features/tasks/service.rs -->
```rust
impl TasksServiceApi for TasksService {
    fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// A task goes only into one of the caller's projects, and counts as
    /// one of its open tasks. Both writes share `conn`, so `create` commits
    /// them together.
    async fn create_in(
        &self,
        conn: &mut PgConnection,
        user: AuthUser,
        input: CreateTaskDto,
    ) -> Result<Task, AppError> {
        self.projects
            .task_opened(&mut *conn, user, input.project_id)
            .await?;
        let row = repo::create(&mut *conn, input.into_new(user.id)).await?;
        Ok(row)
    }
}
```

An override takes the same arguments as the default and returns the row,
never the response: the generated handler turns the row into the response.
It writes no SQL: it calls the data layer through `repo` (the table's
functions) or `crate::db`, and SQL the data layer doesn't generate goes in
a query file. A new error case is a variant in `src/error.rs`.

When a schema change alters a method's signature (a new column in a body,
a new owner), the compiler points at your override; the default needs no
change.

## Adding state to a service

The scaffolded service is a struct holding the pool, and its constructor,
`TasksService::new(pool)`, is what the generated `register_services`
calls:

<!-- source: docs/examples/api/src/features/generated.rs -->
```rust
pub fn register_services(cfg: &mut web::ServiceConfig, pool: &PgPool) {
    cfg.app_data(web::Data::new(projects::ProjectsService::new(pool.clone())));
    cfg.app_data(web::Data::new(tasks::TasksService::new(pool.clone())));
}
```

So add fields freely, and keep `new(pool: PgPool) -> Self`: it builds what
the service needs from the pool. The tasks service holds a port to the
projects feature:

<!-- source: docs/examples/api/src/features/tasks/service.rs -->
```rust
/// The `tasks` service, registered as app data by `register_services`.
pub struct TasksService {
    /// The pool the service reads and writes through.
    pub pool: PgPool,
    /// The projects feature, through this feature's port.
    projects: Arc<dyn ProjectTasks>,
}

impl TasksService {
    /// The service over `pool`, as `register_services` builds it. Give it
    /// what else it needs (a port) here.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        let projects = Arc::new(ProjectTasksPort::new(pool.clone()));
        TasksService { pool, projects }
    }
}
```

An outside system (mail, storage, a queue) works the same way: a trait in
`shared/<name>.rs`, an implementation beside it, and a field built in
`new`.

## Writing a port

A feature may read any table through `crate::db`, but it writes another
feature's data only through a **port**: a trait it declares in its own
`service.rs`, implemented in `app.rs`, the only file that sees both
features. `check-arch` reports a feature that names another feature's
modules (`rule-4`) or another table's `crate::db::<table>::write`
(`cross-feature-write`).

The tasks feature declares what it needs from projects:

<!-- source: docs/examples/api/src/features/tasks/service.rs -->
```rust
/// What the tasks feature needs from the projects feature. `app.rs`
/// implements it.
#[async_trait::async_trait]
pub trait ProjectTasks: Send + Sync {
    /// Counts one more open task on the caller's `project`, on `conn`, or
    /// fails with `404` if the caller has no such project.
    async fn task_opened(
        &self,
        conn: &mut PgConnection,
        user: AuthUser,
        project: ProjectId,
    ) -> Result<(), AppError>;
}
```

`app.rs` implements it with the projects service's `_in` method, on the
connection the tasks service passes, so both writes commit or roll back
together:

<!-- source: docs/examples/api/src/app.rs -->
```rust
#[async_trait::async_trait]
impl ProjectTasks for ProjectTasksPort {
    async fn task_opened(
        &self,
        conn: &mut PgConnection,
        user: AuthUser,
        project: ProjectId,
    ) -> Result<(), AppError> {
        self.projects.open_task_in(conn, user, project).await
    }
}
```

Ports are `#[async_trait]` traits (add the `async-trait` dependency),
because a service holds its port as `Arc<dyn …>`; the generated service
traits use native `async fn`. [Architecture, section
7](architecture.md#7-ports-one-feature-writing-anothers-data) has the
whole example, including the `_in` method the projects service gains.

## A hand-written feature

A feature with no table behind it, such as a dashboard or a webhook, is
yours from the start. It uses the same files as a generated feature (any of
`mod.rs`, `routes.rs`, `handlers.rs`, `dto.rs`, `model.rs`, `repo.rs`,
`service_api.rs` and `service.rs`, and no other), follows the same rules,
and lives beside the generated ones. `rok-sqlc g` never touches it.

The example's `stats` feature answers `GET /api/v1/stats` with the
caller's number of projects and open tasks. Its SQL is a query in
`queries.sql`, so the data layer generates `queries::project_stats`:

<!-- source: docs/examples/api/queries.sql -->
```sql
-- name: ProjectStats :one
SELECT count(*) AS projects, coalesce(sum(open_tasks), 0)::BIGINT AS open_tasks
FROM projects
WHERE owner_id = $1;
```

The service calls it:

<!-- source: docs/examples/api/src/features/stats/service.rs -->
```rust
impl StatsService {
    /// The service over `pool`.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        StatsService { pool }
    }

    /// The caller's projects and open tasks.
    pub async fn summary(&self, user: AuthUser) -> Result<ProjectStatsRow, AppError> {
        let row = queries::project_stats(&self.pool, user.id).await?;
        Ok(row)
    }
}
```

The handler documents itself for the OpenAPI document as the generated ones
do, with `#[utoipa::path]`, its own tag, and `bearer_auth` since it takes
`AuthUser`:

<!-- source: docs/examples/api/src/features/stats/handlers.rs -->
```rust
#[utoipa::path(
    get,
    path = "/api/v1/stats",
    tag = "stats",
    operation_id = "summary",
    responses(
        (status = 200, description = "The caller's statistics.", body = StatsResponse),
        (status = 401, description = "No valid bearer token.", body = ErrorBody),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn summary(
    service: web::Data<StatsService>,
    user: AuthUser,
) -> Result<HttpResponse, AppError> {
    let row = service.summary(user).await?;
    Ok(HttpResponse::Ok().json(StatsResponse::from(row)))
}
```

<!-- source: docs/examples/api/src/features/stats/routes.rs -->
```rust
/// Mounts `GET /api/v1/stats`.
pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("/api/v1/stats").route("", web::get().to(handlers::summary)));
}
```

Three of your scaffolded files wire it in. `features/mod.rs` declares it:

<!-- source: docs/examples/api/src/features/mod.rs -->
```rust
mod generated;
pub use generated::*;

pub mod stats;
```

`app.rs` registers its service and mounts its routes, after the generated
ones:

<!-- source: docs/examples/api/src/app.rs -->
```rust
    configure_all(cfg);
    // The hand-written stats feature: its service, then its routes.
    cfg.app_data(web::Data::new(StatsService::new(pool.clone())));
    stats::routes(cfg);
}
```

and `openapi.rs` lists its handler and its tag in `ApiDoc`, which the
generated document merges into:

<!-- source: docs/examples/api/src/openapi.rs -->
```rust
#[derive(OpenApi)]
#[openapi(
    paths(crate::features::stats::handlers::summary),
    components(schemas()),
    tags((name = "stats", description = "The caller's statistics.")),
)]
pub struct ApiDoc;
```

The response, `StatsResponse`, derives `ToSchema` and is reached from the
path, so it needs no entry in `components(schemas(...))`. A schema no path
refers to goes there.

## What you can't customize in place

Generated files are rewritten on every run, so an edit to one is lost, and
`rok-sqlc g --check` reports it as drift. To change a route, a body, a
response or a validation rule, change its source instead:

| To change | Change |
|---|---|
| a body's fields or rules | the column in a new migration (`VARCHAR(80)` gives `length(max = 80)`) |
| who may call a route, which rows a caller sees | `[api.access]`, `[api.owner]` ([Configuring](configuring.md)) |
| what a response leaves out | `[api.hidden]` |
| the routes a feature serves | `[api.skip]`, `[api.routes]`, `[api] prefix` |
| what a route does | an override in `service.rs` |
| a route rok-sqlc doesn't generate | a hand-written feature |
