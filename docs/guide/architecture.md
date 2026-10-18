# Architecture guidelines

A simple, restricted architecture for Rust services on actix-web, SQLx and
PostgreSQL: **one crate, folders by feature, a fixed set of files per
feature, six rules.** Humans and code generators follow this document
exactly. If a rule gets in your way, change the rule here first, then change
the code.

rok-sqlc generates most of such a service from its schema. `rok-sqlc init`
scaffolds the application skeleton once, and `rok-sqlc g` generates the data
layer (`src/db/`), the shared tools (`src/shared/`) and one **API feature**
per table (`src/features/<table>/`). [Generated and scaffolded
files](layout.md) says which files are rok-sqlc's and which are yours.
`rok-sqlc check-arch` checks the rules below on every build. ADR 0016 records
the decisions behind this guide.

Every Rust sample here comes from [`docs/examples/api`](../examples/api), a
small project started with `rok-sqlc init`, generated from two tables
(`projects` and `tasks`), and compiled by the test suite.

## 1. Principles

1. **Boring and predictable.** Every feature looks the same. If you know one
   feature, you know all of them.
2. **Modular monolith.** One deployable binary, strict feature boundaries. A
   feature can be moved into its own crate or service later without
   rewriting it.
3. **Stateless runtime.** The API can run as many identical copies as
   needed.
4. **Slow work goes to a queue.** Requests stay fast, and a failed side
   effect never fails a request.
5. **Rules are checked by tooling, not by memory.** CI rejects violations
   (`rok-sqlc check-arch`).

## 2. Layout

```text
src/
├─ lib.rs           declares every module `pub`: the application is a library,
│                   so generated code it does not use yet never warns
├─ main.rs          starts the server through the library. Nothing else.
├─ app.rs           builds state, registers services, mounts every feature,
│                   implements ports (the ONLY wiring file)
├─ config.rs        reads env vars into one Config (the ONLY env reader)
├─ error.rs         AppError and its HTTP mapping, shared by all features
├─ db/              generated data layer for the whole schema: rows, typed
│                   ids, enums, all SQL, joins, loaders, pool.rs
├─ shared/          tools with no business meaning: auth.rs (yours);
│                   response, pagination, validation, tenant, health
│                   (generated); mail, storage, queue (yours)
└─ features/
   ├─ mod.rs          `mod generated; pub use generated::*;` and your
   │                  hand-written features
   ├─ generated.rs    declares the features; register_services, configure_all
   └─ <feature>/
      ├─ mod.rs          re-exports `routes`, the service and its trait
      ├─ routes.rs       URL to handler
      ├─ handlers.rs     HTTP in and out only
      ├─ dto.rs          request and response shapes, validation rules
      ├─ model.rs        re-exports the table's rows and typed ids from db
      ├─ repo.rs         re-exports the table's db functions
      ├─ service_api.rs  the service trait, a default per endpoint
      └─ service.rs      yours: the service and its business rules
migrations/  tests/  AGENTS.md  rok-sqlc.toml
```

`src/db/` is infrastructure, like `shared/`. It holds the SQL of every table,
and joins and loaders that span tables, so the rules about `sqlx` and about
other features (rules 2 and 4) don't apply inside it. `db/pool.rs` creates
the pool (`PoolSettings`, `init_pool`) and checks its health
(`check_health`).

Not allowed: new top-level folders under `src/` (only the data layer's,
`shared/`, the features folder and `bin/` exist), new files or folders inside
a feature, and files or folders named `utils`, `helpers` or `common`. If
something does not fit, it is either a `shared/` tool (no business meaning)
or it belongs in a feature. Decide which.

## 3. Where does each piece of code go?

| I am writing... | It goes in | Never in |
|---|---|---|
| A business rule | the feature's `service.rs`, overriding a `service_api.rs` default | handler, repo |
| A new endpoint, request or response shape | the schema and `rok-sqlc.toml` (`[api]`), then `rok-sqlc g` | generated files |
| SQL | the data layer: a migration and `rok-sqlc g`, or a query file | service, handler |
| A new table or column | `migrations/000N_name.sql`, then `rok-sqlc g` | anywhere else |
| An error case | a variant in `error.rs` | a feature folder |
| Code two features need | `shared/` | one feature |
| A call to an outside system | `shared/<name>.rs` behind a trait | a service directly |
| An environment variable | `config.rs` | `std::env` anywhere else |
| Background or slow work | an outbox row written by `service.rs`, handled by a worker | the request path |
| Writing another feature's data | a port declared in your `service.rs`, implemented in `app.rs` | that feature's files |
| A hand-written feature | `features/mod.rs` and `app.rs` | — |

## 4. The six rules

1. In a feature, only `routes.rs` and `handlers.rs` use `actix_web`. Outside
   the features, so may `app.rs`, `main.rs`, `error.rs`, the features'
   `generated.rs`, `shared/` and the data layer.
2. In a feature, only `repo.rs` and `model.rs` use `sqlx`, and `service.rs`
   and `service_api.rs` may name `sqlx::PgPool` and `sqlx::PgConnection` and
   nothing else. Outside the features, so may `app.rs`, `error.rs`, the
   features' `generated.rs`, `shared/` and the data layer.
3. `service.rs` never writes SQL. It calls the data layer's functions and
   traits from `shared/`.
4. A feature never refers to another feature's modules. It may read another
   table through the data layer, but never calls another table's
   `<db_path>::<table>::write`: it writes another feature's data only
   through a port it declares itself, inside the caller's transaction. The
   data layer is exempt: the schema crosses features.
5. `shared/` never refers to the features.
6. No `unwrap()`, `expect()`, `panic!`, `todo!`, `println!` or `dbg!`. Every
   fallible function returns `Result<_, AppError>`.

`lib.rs` only declares the modules, so it needs neither crate. Ports and the
transaction they share are in section 7.

The features are the directories under `api.output` (`src/features/` by
default), and the data layer is the module `api.db_path` names (`crate::db`,
in `src/db/`, by default). `rok-sqlc check-arch` checks rules 1, 2, 4, 5 and
6 and the layout: a feature directory holds only `mod.rs`, `routes.rs`,
`handlers.rs`, `dto.rs`, `model.rs`, `repo.rs`, `service_api.rs` and
`service.rs`; no file or directory is named `utils`, `helpers` or `common`;
and `src/` has no top-level directory but the data layer's, `shared/`, the
features' and `bin/`. Test code is exempt from every rule: files under
`#![cfg(test)]`, items under `#[cfg(test)]`, `#[test]` and `#[<path>::test]`
functions, and `tests/`, which is not read. Each violation prints as
`path:line: rule: message`. Rule 3 is yours to keep.

The cross-feature write part of rule 4 is reported as `cross-feature-write`
and the layout as `layout`. `check-arch` reads `src/` with a Rust parser, so
comments and strings never count. A path counts wherever it appears: a
`use`, an expression, a type, a pattern, a `#[derive(...)]` list or an
attribute like `#[actix_web::main]`. A path reached through a `use` binding
that already breaks a rule is reported once, at the `use`. Rule 6 counts
`unwrap` and `expect` called as methods (`x.unwrap()`) or through a path
(`Option::unwrap(x)`).

Any violation exits with code 1. `-l`/`--lenient` turns layout violations
into warnings while you move a project over.

**Conventions.** Handlers are thin: parse, call one service method, answer,
with no business `if`. One service method does one thing; name it as a verb.
DTOs are separate from rows, and a response never carries a secret: list
such columns in `[api.hidden]`. Money is integer minor units, time is UTC,
and ids are UUIDs (`primary_key_ids = "uuid_v7"`, which `init` sets).

## 5. A feature, file by file

`rok-sqlc g` writes every file of a feature but `service.rs` on every run.
The feature for `tasks` serves `/api/v1/tasks`:

<!-- source: docs/examples/api/src/features/tasks/routes.rs -->
```rust
/// Mounts every `tasks` endpoint under `/api/v1/tasks`.
pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/v1/tasks")
            .app_data(path_config())
            .app_data(query_config())
            .route("", web::get().to(handlers::list))
            .route("", web::post().to(handlers::create))
            .route("/{id}", web::get().to(handlers::get_one))
            .route("/{id}", web::patch().to(handlers::update))
            .route("/{id}", web::delete().to(handlers::remove)),
    );
}
```

A handler extracts the caller, the path, the query and a validated body,
calls one service method, and maps the row to its response:

<!-- source: docs/examples/api/src/features/tasks/handlers.rs -->
```rust
/// `POST /api/v1/tasks`: creates a row, `201` with its `Location`.
pub async fn create(
    service: web::Data<TasksService>,
    user: AuthUser,
    body: ValidatedJson<CreateTaskDto>,
) -> Result<HttpResponse, AppError> {
    let row = service.create(user, body.into_inner()).await?;
    let location = format!("/api/v1/tasks/{}", row.id);
    let response = TaskResponse::from(row);
    Ok(HttpResponse::Created()
        .insert_header((header::LOCATION, location))
        .json(response))
}
```

`dto.rs` holds the bodies. Their validation rules come from the schema
(`VARCHAR(200)` gives `length(max = 200)`), and an owned table's owner
column is left out: the service sets it from the caller.

<!-- source: docs/examples/api/src/features/tasks/dto.rs -->
```rust
/// `POST /api/v1/tasks`'s body, checked by the rules of [`NewTask`].
/// A field for a defaulted column may be left out.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, validator::Validate)]
pub struct CreateTaskDto {
    /// The `project_id` column.
    pub project_id: ProjectId,
    /// The `title` column.
    #[validate(length(max = 200, message = "title must be at most 200 characters"))]
    pub title: String,
    /// The `done` column.
    pub done: Option<bool>,
    /// The `created_at` column.
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}
```

`model.rs` and `repo.rs` don't hold rows or SQL of their own. They re-export
the table's part of the data layer, so the feature reads like the original
guidelines' `model.rs` and `repo.rs`:

<!-- source: docs/examples/api/src/features/tasks/repo.rs -->
```rust
pub use crate::db::tasks::listing::*;
pub use crate::db::tasks::read::*;
pub use crate::db::tasks::relationships::*;
pub use crate::db::tasks::write::*;
```

## 6. Services and transactions

`service_api.rs` declares the feature's service trait, `TasksServiceApi`.
It has one method per endpoint, each with a working default over the data
layer. Its only required method is `pool`. Every write comes in two parts:

- `create_in(&self, conn: &mut PgConnection, …)` holds the logic on a
  connection the caller owns. It is the one to override.
- `create(&self, …)` opens a transaction, calls `create_in`, and commits.

<!-- source: docs/examples/api/src/features/tasks/service_api.rs -->
```rust
    /// What [`create`](Self::create) does, on `conn`: the caller's connection or
    /// transaction. Override this one to change it.
    async fn create_in(
        &self,
        conn: &mut PgConnection,
        user: AuthUser,
        input: CreateTaskDto,
    ) -> Result<Task, AppError> {
        let row = repo::create(&mut *conn, input.into_new(user.id)).await?;
        Ok(row)
    }

    /// `POST /api/v1/tasks`: creates a row and returns it.
    /// Runs [`create_in`](Self::create_in) in a transaction of its own.
    async fn create(&self, user: AuthUser, input: CreateTaskDto) -> Result<Task, AppError> {
        let mut tx = self.pool().begin().await?;
        let row = self.create_in(&mut tx, user, input).await?;
        tx.commit().await?;
        Ok(row)
    }
```

Methods take the request's DTOs and return rows; the handlers turn rows into
responses. On an owned table (`[api.owner]`) every method takes the caller
(`user: AuthUser`), reads only the caller's rows, and `update_in` and
`remove_in` first lock the caller's row (`SELECT … FOR UPDATE`) and then
update or delete it, so another caller's row is a `404`. On a tenant-scoped
table every method takes the tenant first.

`service.rs` is scaffolded: written once, then yours. It holds the service
struct, `TasksService { pool }`, its constructor `TasksService::new(pool)`
(which the generated `register_services` calls), and an empty `impl
TasksServiceApi for TasksService` with only `pool`. A business rule is an
override there. Because every other method keeps its default, an endpoint a
later schema change adds arrives working, and regeneration never breaks your
file.

## 7. Ports: one feature writing another's data

A feature may read any table through `crate::db`. It writes another
feature's data only through a **port**: a trait it declares in its own
`service.rs`. `app.rs` implements the port by calling the other service's
`_in` method on the caller's connection, so both writes commit or roll back
together.

In the example, a task may only go into one of the caller's projects, and
each project counts its open tasks. The tasks feature declares what it needs
from projects:

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

and overrides `create_in` to call it on the same connection before creating
the task:

<!-- source: docs/examples/api/src/features/tasks/service.rs -->
```rust
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
```

The projects service gains the `_in` method the port calls. Its SQL lives in
the data layer, in a query file (`queries.sql`), not in the service:

<!-- source: docs/examples/api/src/features/projects/service.rs -->
```rust
    pub async fn open_task_in(
        &self,
        conn: &mut PgConnection,
        user: AuthUser,
        id: ProjectId,
    ) -> Result<(), AppError> {
        let updated = queries::open_task(&mut *conn, id, user.id).await?;
        if updated == 0 {
            return Err(AppError::NotFound("no such project".to_owned()));
        }
        Ok(())
    }
```

`app.rs`, the only file that sees both features, implements the port:

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

The generated `register_services` builds every service with `new(pool)`:

<!-- source: docs/examples/api/src/features/generated.rs -->
```rust
pub fn register_services(cfg: &mut web::ServiceConfig, pool: &PgPool) {
    cfg.app_data(web::Data::new(projects::ProjectsService::new(pool.clone())));
    cfg.app_data(web::Data::new(tasks::TasksService::new(pool.clone())));
}
```

so a service that needs a port builds it in `new`, from the implementation
`app.rs` provides. The port sits behind `Arc<dyn …>`, which is why ports are
`#[async_trait]` traits while the generated service traits use native `async
fn`:

<!-- source: docs/examples/api/src/features/tasks/service.rs -->
```rust
    pub fn new(pool: PgPool) -> Self {
        let projects = Arc::new(ProjectTasksPort::new(pool.clone()));
        TasksService { pool, projects }
    }
```

Use traits only for ports and outside systems, never around data-layer
functions. A change that needn't be atomic with the request (an email, a
search index) is an outbox row instead (section 9). `check-arch` reports a
feature that calls another table's `crate::db::<table>::write` as
`cross-feature-write`.

## 8. Errors and the wire standard

Every error is an `AppError` variant from the scaffolded `error.rs`,
answered as `{"error": {"code", "message", "details"?}}` with a snake_case
code. A database error maps by itself: a unique or foreign-key violation is
`409 conflict` naming the constraint, a check violation `422
validation_failed`, and anything unexpected `500 internal_error`, logged
and never detailed.

<!-- source: docs/examples/api/src/error.rs -->
```rust
    pub fn code(&self) -> &'static str {
        match self {
            AppError::BadRequest(_) => "bad_request",
            AppError::Unauthorized(_) => "unauthorized",
            AppError::Forbidden(_) => "forbidden",
            AppError::NotFound(_) => "not_found",
            AppError::Conflict(_) => "conflict",
            AppError::PayloadTooLarge(_) => "payload_too_large",
            AppError::Validation { .. } => "validation_failed",
            AppError::PreconditionRequired(_) => "precondition_required",
            AppError::VersionConflict(_) => "version_conflict",
            AppError::Internal(_) => "internal_error",
        }
    }
```

A success answer is the bare object: `200`, or `201` with `Location` for a
create, and `204` with no body for a delete. A list answers `{items, page,
per_page, total}` from offset pages (`?page=&per_page=`, 20 a page by
default, at most 100), not keyset cursors; keyset pagination stays available
in the data layer. A relationship's rows (`GET /{key}/<name>`) answer a
bare JSON array. A malformed path, query or body is `400 bad_request`, and
a well-formed body that breaks its rules `422 validation_failed` with the
details by field. The README's [Generated APIs](../../README.md#generated-apis)
section lists every route.

## 9. Runtime checklist

- Stateless.
- Configuration from the environment only, read in `config.rs`.
- An explicit pool size and timeouts (`DB_MAX_CONNECTIONS`,
  `DB_ACQUIRE_TIMEOUT_SECS` and the other `PoolSettings` variables).
- `/health/live` (the process answers) and `/health/ready` (the database
  answers), outside the API prefix.
- Offset pagination with a total, as the wire standard requires.
- `Idempotency-Key` on retried mutations.
- Migrations as a separate step, kept backward compatible.
- Structured logs with a request id (`tracing`).
- An outbox table written in the same transaction as the change, processed
  by a `worker` binary with `FOR UPDATE SKIP LOCKED`.

## 10. Testing

- **Service rules:** `#[sqlx::test]` against a real PostgreSQL, with fakes
  for ports and outside traits.
- **Data layer:** generated, and tested by rok-sqlc. Test your query files
  with `#[sqlx::test]`.
- **API:** `tests/<feature>.rs` with `actix_web::test`, building the
  application with `app::configure`.
- **Shared:** unit tests.

A feature is done with one service test per business rule and one API test
per route you changed: the happy path plus one failure.

## 11. Adding a feature

1. Write a migration for the table.
2. Set its `[api]` entries in `rok-sqlc.toml`: access, owner, hidden
   columns, search and sort columns, skipped endpoints.
3. Run `rok-sqlc g`. The feature appears in `src/features/<table>/` and in
   `generated.rs`, and `app.rs` already mounts it.
4. Put its business rules in the new `service.rs`, add error variants to
   `error.rs`, write tests, and run the checks.

A hand-written feature (one with no table) uses the same eight files,
declared with `pub mod <name>;` in `features/mod.rs` and mounted in
`app.rs`.

## 12. Automated checks

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo
test`, `rok-sqlc g --check` (the generated code matches the schema) and
`rok-sqlc check-arch`. `init` lists the clippy lints that deny `unwrap`,
`expect` and `panic` at the end of `rok-sqlc.toml`.

## 13. When to split

- More than about five developers, or slow builds: a `core` crate plus one
  crate per feature.
- Background work: add a `worker` binary in `src/bin/`.
- Different scaling needs: extract the feature behind its port.

Otherwise, stay as you are.

## 14. Do and don't

**Do:** keep handlers short, write a new migration for every schema change,
put business rules in `service.rs` overrides, and return `Result`
everywhere.

**Don't:** edit a generated file, create `utils`, `helpers` or `common`, add
a speculative trait, read the environment outside `config.rs`, write another
feature's data except through a port, do slow work in a request, or edit an
applied migration.
