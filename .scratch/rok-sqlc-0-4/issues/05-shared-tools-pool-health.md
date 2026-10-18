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

**Status:** ready-for-agent

- [ ] Generator tests for each file's presence and absence, plus `PoolSettings::for_hardware` unit test in the generated code via the fixture.
- [ ] Fixture (the new `tests/api-app`, created here or in 06): `/health/live` and `/health/ready` against PostgreSQL, the latter 503 when the pool can't connect.
