# Generated APIs: features over a generated data layer, one wire standard, two templates

An API built on the generated code repeats the same work for every table: routes, handlers that extract and validate, a service for business rules, an OpenAPI document, and the mapping of database errors to HTTP. rok-sqlc generates this as one **API feature** per table when `api_output` is set. It generates for actix-web first and axum second, and it follows the maintainer's architecture guidelines (`docs/guide/architecture.md`). Every generated API speaks one wire standard, taken from the axum starter's OpenAPI document. Generated code and user edits never share a file. The **service**, where business rules live, is a scaffolded file the user owns. A generated trait with default methods joins it to the regenerated code, so neither can break the other.

## Project layout

The layout is the architecture guidelines' single-crate layout, with these amendments:

- `src/db/` is the generated data layer for the whole schema: rows, typed ids, enums, all SQL, joins, loaders, and `db/pool.rs` (`PoolSettings`, `init_pool`, `check_health`). It is infrastructure, like `shared/`. The rules "only `repo.rs`/`db.rs` import `sqlx`" and "no cross-feature imports" do not apply inside it, because the schema crosses features.
- `rok-sqlc init` scaffolds the user-owned skeleton once: `main.rs`, `app.rs` (the only wiring file, calling the generated `register_services` and `configure_all`), `config.rs` (the only environment reader), `error.rs` (`AppError` with the variants generated code uses and the `sqlx` error mapping), `shared/mod.rs`, `shared/auth.rs` (`AuthUser`, `AdminUser`, HS256 JWT verification), `features/mod.rs` (`mod generated; pub use generated::*;`), and `AGENTS.md`.
- `shared/` holds regenerated tools: `response.rs`, `validation.rs`, `pagination.rs`, `tenant.rs`. It also holds the user's own tools, such as mail, storage and queue.
- Each generated feature has exactly these files:

| File | Owner | Contents |
|---|---|---|
| `mod.rs` | generated | re-exports `routes`, the service and the trait |
| `routes.rs` | generated | URL to handler mapping |
| `handlers.rs` | generated | parse, call one service method, return |
| `dto.rs` | generated | `CreateUserDto`, `UpdateUserDto` (all optional), `UserResponse` (`From<User>`, minus hidden columns), list query |
| `model.rs` | generated | re-exports the table's rows and typed ids from `db` |
| `repo.rs` | generated | re-exports the table's `db` functions |
| `service_api.rs` | generated | the `UsersServiceApi` trait with defaults |
| `service.rs` | scaffolded | `UsersService { pool }` and the user's overrides |

No other file may be added to a generated feature. A feature's business logic lives in `service.rs`, and outside systems sit behind `shared/` traits. `rok-sqlc check-arch` enforces this layout and the guidelines' rules 1, 2, 4, 5 and 6, and reports violations as `path:line`.

## Services, transactions and ports

`UsersServiceApi` has one method per endpoint, and its only required method is `fn pool(&self) -> &PgPool`. Each write comes in two parts:

- `create_in(&self, conn: &mut PgConnection, …)` holds the logic, and is where the user overrides;
- `create(&self, …)` opens a transaction, calls `create_in`, and commits.

An endpoint added by a later schema change arrives with a working default. A feature may read another table through `crate::db`. It writes another feature's data only through a **port** it declares in its own `service.rs`. The port is implemented in `app.rs` by calling the other service's `_in` method on the caller's connection, so both changes commit together. Ports are `#[async_trait]` traits behind `Arc<dyn …>`. The generated service trait uses native `async fn`. A change that needn't be atomic goes through the outbox. `check-arch` reports a feature calling another table's `db::<table>::write`.

## Wire standard

Both templates follow the axum starter's OpenAPI document:

- **Success:** a bare object; 201 for create, 204 for delete.
- **Errors:** `{"error": {"code", "message", "details"?}}` with snake_case codes:

| Code | Status | Meaning |
|---|---|---|
| `bad_request` | 400 | malformed body, path or query |
| `unauthorized` | 401 | no valid credentials |
| `forbidden` | 403 | not allowed |
| `not_found` | 404 | no such row |
| `conflict` | 409 | unique or foreign-key violation, naming the constraint |
| `payload_too_large` | 413 | body over the limit |
| `validation_failed` | 422 | failed rules, with validator's details; also a check violation |
| `precondition_required` | 428 | `If-Match` missing on a versioned table |
| `version_conflict` | 412 | stale `If-Match` |
| `internal_error` | 500 | anything else; logged, never detailed |

  The 0.3 extractors' body changes to this shape in 0.4.0.
- **Lists:** `GET ?page=&per_page=&q=&sort=&order=` returns `{items, page, per_page, total}`. Page defaults to 1 and `per_page` to 20, capped at 100. `[api.search]` names the columns `q` matches (`ILIKE`, escaped). `[api.sort]` names the sort columns, generating `XSort`; `order` defaults to `desc`. `POST /search` with per-field filters is opt-in (`[api] search = true`).
- **Routes:** under `[api] prefix` (default `/api/v1`), plural kebab-case route bases, `{id}` paths, PATCH for updates. `/health/live` and `/health/ready` sit outside the prefix. The docs are `/api-docs/openapi.json` and Swagger UI at `/docs`. Tags are lowercase kebab-case.

  Unique lookups (`/by-<column>/{value}`), relationship routes (`?include=`, has-many, many-to-many sync, attach and detach) and soft-delete routes (`/restore`, `?permanent=true`) work as first decided. `[api.skip]` drops any route.
- **OpenAPI:** every generated handler carries `#[utoipa::path]`, and every DTO derives `ToSchema` or `IntoParams`. A generated `ApiDoc` with the `bearer_auth` addon merges with a scaffolded one for hand-written features.
- **Access:** `[api.access]` sets each table to `public`, `user` (the default; `AuthUser`) or `admin` (`AdminUser`). `[api.owner]` names an owner column. Every query is then scoped to the caller, the owner is set on create, and another caller's row is a 404. Tenant-scoped tables take the tenant from a `Tenant` extractor; a missing tenant is a 500 wiring bug. Versioned tables use `ETag` and `If-Match`.
- **Hidden columns:** `[api.hidden]` columns are left out of `UserResponse`. Filters, sorts and lookups on them are rejected, and they get no `by-` route.
- **Ids:** `primary_key_ids` adds `cuid2` (`cuid::cuid2()`, text keys) beside `uuid_v4`, `uuid_v7` and `ulid`. The templates use `UUID` keys with `uuid_v7`.

## Templates

`templates/actix-api/` (first) and `templates/axum-api/` (second) live in this repository, and `rok-sqlc init --template actix` copies one into a new project. Both have the same layout. They differ only in `routes.rs`, `handlers.rs`, the extractors, and the server setup in `main.rs` and `app.rs`. Each template generates from one `schema.sql`, derived from the axum starter's document. Its contract test checks the generated OpenAPI document against that document for every CRUD path. The starter's auth surface (login, 2FA, OAuth, passkeys, uploads) is not derivable from SQL. It is a later milestone, ported as hand-written features. CI builds both templates and runs their tests.

## Considered Options

- **Scaffold every file once.** Rejected: the API stops following the schema after the first run.
- **Regenerate everything, customize only through configuration.** Rejected: business rules don't fit configuration and would be lost on regeneration.
- **Plain service functions in a scaffolded file.** Rejected: an endpoint added by a schema change would call a function the user's file lacks, and the build would break.
- **SQL in each feature's `repo.rs`.** Rejected: joins and loaders span tables, so every relationship would need a port.
- **The `{success, data, meta}` envelope, or a configurable `style`.** Rejected: the maintainer chose the axum starter's OpenAPI document as the standard, and a second style would double every template and test.
- **The actix starter's workspace and `#[module]` macro.** Rejected: the guidelines' single crate keeps the two templates identical apart from the framework edge.
- **Keyset pagination by default.** Rejected: the standard is offset pages with a total. Keyset stays available in the data layer.
