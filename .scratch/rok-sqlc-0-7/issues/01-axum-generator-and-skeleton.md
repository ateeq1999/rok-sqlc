# 01: The generator emits axum, and `init --framework axum`

**What to build:** decisions A1, A2, A3 and A5.

- **Config:** `[api] framework = "actix" | "axum"` (default `actix`), parsed and validated. An unknown value fails the config check with the allowed values. `extractors` must be consistent: `framework = "axum"` implies the axum `ValidatedJson`. Fail clearly on a conflicting `extractors = ["actix_web"]`, or infer it; pick one and document it.
- **Generated, per feature, under axum:**
  - `routes.rs`: a function returning `Router<AppState>` (or generic over `S` with `FromRef`; pick the clearest) with the same paths and methods as actix, relation routes included.
  - `handlers.rs`: `State<Arc<XService>>`, `Path`, `Query`, `ValidatedJson`, `AuthUser`/`AdminUser`/`Tenant`, `ETag`/`If-Match` handling, and the same statuses and bodies.
  - The same `#[utoipa::path]` attributes.

  `dto.rs`, `model.rs`, `repo.rs`, `service_api.rs` and the scaffolded `service.rs` must be byte-identical to actix's output for the same schema. Add a test that generates both and diffs those files.
- **Generated root (`features/generated.rs`):**
  - `Services` (one `Arc<XService>` per feature, `Clone`), `register_services(pool) -> Services`, and `impl FromRef<Services> for Arc<XService>`;
  - `configure_all` becomes a `routes() -> Router<AppState>` merging every feature (or the axum equivalent);
  - `GeneratedApiDoc` unchanged.
- **Skeleton (`init --framework axum`, scaffolded once):**
  - `main.rs`, `app.rs`: `AppState { pool, services, … }` deriving `FromRef` (the axum `macros` feature), the router, tower-http layers (trace, CORS, request id, timeout, body limit), Swagger UI on `/docs/`, `/api-docs/openapi.json`, `/health/live|ready`, and graceful shutdown.
  - `config.rs` and `error.rs` (`AppError: IntoResponse`, same JSON error body and codes).
  - `shared/`: `auth.rs` (`AuthUser`/`AdminUser` as `FromRequestParts` with the same HS256 verification the plain actix skeleton does), `tenant.rs`, `pagination.rs`, `response.rs`, `validation.rs`, `health.rs`.
  - `features/mod.rs`, `openapi.rs`, `AGENTS.md`, and a `Cargo.toml` manifest hint with axum 0.8, tower-http 0.6 and tokio.
  - The existing `init` skeleton machinery decides where these live: framework-specific skeleton files beside the actix ones.
- **`init --framework axum`** writes the skeleton and `rok-sqlc.toml` with `framework = "axum"`. `init --template axum` still answers "unknown template" until ticket 03.

**Blocked by:** None

**Status:** ready-for-agent

- [ ] Unit/CLI tests: config parsing, the framework-independent files identical across frameworks, and the generated axum route/handler shapes (expected-file tests like actix's).
- [ ] A fresh `init --framework axum` + `g` project with a sample schema builds and is clippy-clean on Rust 1.94, and its `check-arch` passes (extend `tests/fixture_compile/init.rs` or add a sibling). Keep the PostgreSQL wire suite for ticket 02.
- [ ] Gates: fmt, clippy, `cargo test --locked`, `+1.94` against Debian, the `+1.88` build.
