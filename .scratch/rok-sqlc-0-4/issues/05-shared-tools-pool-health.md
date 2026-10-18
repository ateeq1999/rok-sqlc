# 05: Generated shared tools, the pool, and health

**What to build:** With `api.output` set, generate (regenerated, `@generated`) the infrastructure ADR 0016 names, at the paths ticket 04 fixed:
- `db/pool.rs`: `PoolSettings` (public fields; `Default` max 20, min 5, acquire 3 s, idle 600 s, lifetime 1800 s; `for_hardware(cpu_cores, spindles)` sets `max_connections = cores * 2 + spindles`), `init_pool(url, &settings)`, and `check_health(&pool)` (`SELECT 1`, returning the error). It reads no environment variables and logs with `tracing`.
- `shared/response.rs`: the wire standard's error body types (re-exported or moved from ticket 01) and `PaginatedResponse<T> {items, page, per_page, total}`.
- `shared/pagination.rs`: `ListQuery {page, per_page, q, sort, order}` parts and `Pagination` (page defaults to 1, `per_page` to 20, clamped 1 to 100, `limit()`/`offset()` as `i64`), plus `SortOrder {asc, desc}` (default `desc`).
- `shared/validation.rs`: re-exports the actix `ValidatedJson` and `BodyRejection` from `db::http`.
- `shared/tenant.rs`: a `Tenant<T>` extractor reading what auth middleware stored in the request extensions. Missing is a 500 with code `internal_error` (a wiring bug) via `AppError::Internal`.
- `shared/health.rs`: `/health/live` (`{"status":"ok"}`) and `/health/ready` (`check_health`: 200 `{"status":"ready"}`, or 503 with code `service_unavailable`).

These files are generated individually into `shared/`, which holds user files too (`shared/mod.rs` is scaffolded by ticket 04 and declares them). Paths inside `db` follow `api.db_path`. Generate nothing of this without `api.output`.

**Blocked by:** 03, 04

**Status:** resolved

- [x] Generator tests for each file's presence and absence, plus `PoolSettings::for_hardware` unit test in the generated code via the fixture.
- [x] Fixture (the new `tests/api-app`, created here or in 06): `/health/live` and `/health/ready` against PostgreSQL, the latter 503 when the pool can't connect.

## Comments

Resolved on branch `rok-sqlc-0-4-05`.

**Generator.** `src/generator/api_shared.rs` (templates as consts, written against `crate::db::`, rewritten to `api.db_path`). With `api.output`: `db/pool.rs` under `output` (declared by `db/mod.rs`, which then hints `tracing = "0.1"`), and `shared/{health,pagination,response,tenant,validation}.rs` in `api.output`'s parent `/shared` (`api_shared::shared_dir`). All carry `@generated`. Stale detection still walks only `output`: dropping `[api]` removes `db/pool.rs`, but nothing scans `shared/` or `api.output` — ticket 06 should decide stale handling for the features directory (and could extend it to `shared/`).

**Generated API** (for 06):
- `db::pool`: `PoolSettings` (pub fields, `Default`, `for_hardware(cores, spindles)`, at least 1), `init_pool(url, &settings)`, `check_health(&pool) -> Result<(), sqlx::Error>`.
- `shared::pagination`: `ListQuery<S = String> { page, per_page, q, sort: Option<S>, order: Option<SortOrder> }` (`pagination()`, `sort_order()`), `Pagination { page, per_page }` (`new(Option, Option)`, `limit()`, `offset()`, consts `DEFAULT_PER_PAGE`/`MAX_PER_PAGE`), `SortOrder { Asc, Desc }` (default `Desc`). `From<SortOrder> for db::search::Direction`, `From<Pagination> for db::search::Page`. Use `ListQuery<XSort>` for a table's sort key; don't `#[serde(flatten)]` it (serde_urlencoded can't parse numbers through flatten). A bad query string is actix's default 400, not the wire body; 06 needs a `QueryConfig` error handler.
- `shared::response`: `PaginatedResponse<T> { items, page, per_page, total: i64 }` (`new(items, pagination, total)`, `map(f)`), re-exports `ErrorBody`, `ErrorPayload`.
- `shared::validation`: re-exports `ValidatedJson`, `BodyRejection`.
- `shared::tenant::Tenant<T: Clone + 'static>(pub T)`, read from request extensions; missing → `AppError::Internal` (500 `internal_error`).
- `shared::health`: `routes(cfg)`, `live`, `ready`, `SERVICE_UNAVAILABLE`.

**Fixture `tests/api-app`.** Checked in: `Cargo.toml`, `Cargo.lock` (seeded from fixture-app's), `migrations/0001_notes.sql` (one `notes` table; extend it), `src/lib.rs` (lint gate + `unwrap_used`/`expect_used`/`panic` denied; declares `app, config, db, error, features, shared`), `src/main.rs` (uses the lib instead of `init`'s, which would compile the modules twice and hit `dead_code`), `src/features/generated.rs` (a **stand-in** with the two fixed fns — 06 deletes it and gitignores the generated one), `tests/health.rs`, `tests/shared.rs`. The harness in `tests/fixture_compile.rs` (`generate_api_app`, own `API_APP` mutex) runs `rok-sqlc init` into a temp dir, copies its `src/` (all but `main.rs`) over the fixture every run, then generates with `api.output = src/features` and `init`'s settings. Tests: `api_app_passes_the_lint_gate`, `api_app_passes_its_tests` (`cargo test` on the whole crate), `api_app_generated_code_is_rustfmt_clean`. To extend: add tables to `migrations/`, set `ApiConfig` fields in `generate_api_app`, add files under `tests/`. If 06 needs edited skeleton files (e.g. ports in `app.rs`), the copy step overwrites them; adjust it.

**Deviations.** The end-to-end compile found two scaffolded-template lint failures (`must_use_candidate`): added `#[must_use]` to `AppError::code` and `JwtKeys::new`. `tests/release_metadata.rs` checks `api-app`'s `rust-version` too. README and CHANGELOG note the generated files.
