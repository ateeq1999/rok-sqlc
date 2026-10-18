# 06: API features: layout, services, and CRUD

**What to build:** The heart of ADR 0016. For every generated table not in `api.exclude` (join tables excluded), generate the feature under `api.output/<module>/`.

**The eight files:**

| File | Contents |
|---|---|
| `mod.rs` | re-exports `routes`, the service struct, and the trait |
| `routes.rs` | `pub fn routes(cfg: &mut web::ServiceConfig)`: one `web::scope(<prefix><route base>)` |
| `handlers.rs` | `list`, `create`, `get_one`, `update`, `remove`: parse, call one service method, return |
| `dto.rs` | `CreateUserDto` (validated like `NewUser`), `UpdateUserDto` (all optional, validated like the patch), `UserResponse` (`From<User>`), `ListUsersQuery` |
| `model.rs` | re-exports the rows and typed ids from `db` |
| `repo.rs` | re-exports the table's `db` functions |
| `service_api.rs` | the `UsersServiceApi` trait, native `async fn` |
| `service.rs` | scaffolded: `pub struct UsersService { pub pool: PgPool }` and `impl UsersServiceApi for UsersService { fn pool(&self) -> &PgPool { &self.pool } }` |

**The service trait:** each write endpoint has a `create_in(&self, conn: &mut PgConnection, …)` holding the logic and defaulting to `db`, and `create(&self, …)`, which opens a transaction, calls `_in`, and commits. Reads take `&self` and use the pool. Everything returns `Result<_, AppError>`.

**Root files:**
- `api.output/generated.rs` (regenerated) declares each feature with `#[path = "users/mod.rs"] pub mod users;`. It also holds `pub fn register_services(cfg, pool)` (`app_data` per service) and `pub fn configure_all(cfg)`.
- `api.output/README.md` (regenerated) maps which files are generated and which are yours.

**Endpoints:** route base is the kebab-case module name, overridden by `api.routes`, under `api.prefix`:
- `GET /` with `?page&per_page&q&sort&order` → `{items, page, per_page, total}`. `q` uses ILIKE, escaped, over the `api.search` columns. `sort` is over `api.sort` (default: the primary key, plus `created_at` if present), generating `XSort`. New `db` functions may be needed; add them in the data layer.
- `POST /` → 201 with `Location`.
- `GET /{key…}`, `PATCH /{key…}` and `DELETE /{key…}` (204). Composite keys go in the path in order. A table without a key gets only list and create.
- `api.skip` drops endpoints and their trait methods.

**Errors** go through `AppError` per the wire standard.

**Tests:** a new fixture crate `tests/api-app` (actix-web 4, generated with `api.output` from a schema covering a UUID key with `uuid_v7`, a composite key, a key-less table, search and sort config, and a skip). Its tests use `actix_web::test` against PostgreSQL: status codes and bodies for every endpoint and failure, paging metadata, `q`/`sort`, and a service override in the scaffolded `service.rs` surviving regeneration. Add the regeneration scenario: a schema change adds an endpoint and the user's file stays untouched. Wire it into `tests/fixture_compile.rs` like the other fixtures (lint gate included). Also include an end-to-end compile check of `rok-sqlc init` plus generation.

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] Every endpoint and failure case is covered in `tests/api-app` against PostgreSQL, and the lint gate passes.
- [ ] Scaffolded `service.rs` overrides survive regeneration and a schema change, and output without `api.output` is unchanged.
