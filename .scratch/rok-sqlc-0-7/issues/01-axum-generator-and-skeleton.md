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

**Status:** resolved

- [x] Unit/CLI tests: config parsing, the framework-independent files identical across frameworks, and the generated axum route/handler shapes (expected-file tests like actix's).
- [x] A fresh `init --framework axum` + `g` project with a sample schema builds and is clippy-clean on Rust 1.94, and its `check-arch` passes (extend `tests/fixture_compile/init.rs` or add a sibling). Keep the PostgreSQL wire suite for ticket 02.
- [x] Gates: fmt, clippy, `cargo test --locked`, `+1.94` against Debian, the `+1.88` build.

## Comments

Done on `rok-sqlc-0-7-01`. Actix output is byte-identical to before (checked against the `rok-sqlc-0-7` binary on `tests/api-app`'s migrations with OpenAPI on; the milestone-1 record covers it off).

**Config.** `ApiConfig::framework: ApiFramework { Actix (default), Axum }`, `[api] framework = "actix" | "axum"` (unknown values list both). Extractors are *checked, not inferred*: the API needs the framework's extractor in `extractors` (`ApiFramework::extractor()`); `framework = "axum"` with only `actix_web` fails naming `axum`. Both extractors together are fine.

**The seam.** `src/generator/api/framework.rs`: `impl ApiFramework` gives the handler pieces (`handler_uses`, `service_param`, `path`/`query` -> `Extracted { param(uses), value, push_let }`, `returns`, `push_answer`, `push_no_content`); `handlers.rs` and `relation_routes.rs` render once and ask it. What differs whole is in `api/axum.rs` (`routes.rs`, `generated.rs`) and `api/axum_shared.rs` (`shared/health.rs`, `tenant.rs`, the head of `validation.rs`), dispatched from `handlers::render_routes`, `root::render_generated` and `shared::render`. `Feature::framework` carries it.

**Generated axum shapes.**
- `features/<f>/routes.rs`: `pub fn routes() -> Router<AppState>` (`crate::app::AppState`), full URLs, one `.route` per path with chained methods.
- `handlers.rs`: `State(service): State<Arc<XService>>`, `Path(key): Path<K>` / `Query(query): Query<Q>` from `crate::shared::validation` (axum's wrapped: rejection -> `AppError::BadRequest`), `ValidatedJson`, `IfMatch`, `AuthUser`/`AdminUser`/`Tenant` as `FromRequestParts`; returns `Result<Response, AppError>`. `?include=` arms needing a stricter caller or a tenant take `user|admin|tenant: Result<_, AppError>` params and `?` them in the arm.
- `features/generated.rs`: `#[derive(Clone)] pub struct Services { pub <module>: Arc<XService>, .. }`, `register_services(pool: &PgPool) -> Services`, `impl FromRef<Services> for Arc<XService>` and `impl FromRef<AppState> for Arc<XService>` (reads `state.services`, so `AppState` must have a `services: Services` field), `pub fn routes() -> Router<AppState>` merging every feature. `GeneratedApiDoc` unchanged.
- `shared/health.rs`: `routes<S>() -> Router<S>` where `PgPool: FromRef<S>`; same `liveness`/`readiness`.
- `db/mod.rs`: the Swagger UI hint names the `axum` feature.

**Skeleton.** `src/init/templates/axum/` holds the axum-only files (`rok-sqlc.toml`, `app.rs`, `error.rs`, `main.rs`, `features/mod.rs`, `shared/auth.rs`); `AGENTS.md`, `config.rs`, `lib.rs`, `openapi.rs`, `shared/mod.rs` are shared (`AGENTS.md` takes a `{wiring}` placeholder). `init::plan(dir, krate, framework)`, `init::init(dir, framework)`; CLI `init -f/--framework actix|axum` (conflicts with `--template`). `app.rs`: `#[derive(Clone, FromRef)] pub struct AppState { pool, services, keys: Arc<JwtKeys> }`, `AppState::new(pool, JwtKeys)`, `pub fn router(state) -> Router` (health, `SwaggerUi::new("/docs")`, `routes()`, `DefaultBodyLimit` 2 MiB, `TimeoutLayer::with_status_code(408, 30s)`, `CorsLayer::new()`, request id, trace), `run` with `axum::serve(..).with_graceful_shutdown`. Auth extractors need `Arc<JwtKeys>: FromRef<S>`.

**For ticket 02.**
- `tests/api-app-axum/` exists with only `Cargo.toml` (all deps the fixture needs: axum macros, tokio, tower-http, utoipa-swagger-ui axum, plus api-app's), a placeholder `src/lib.rs`, and a `Cargo.lock` (made from api-app's with `cargo +1.94 update -w`). Fill in `src/`, `tests/` and `rok-sqlc.toml`.
- How to build an axum project in tests: `tests/fixture_compile/init_axum.rs` (init --framework axum, a richer migration, g, check-arch, `write_manifest_for(dir, name, left_out, "axum", &fixture_dir)`, clippy, a TCP-based docs/health/400 test). Library-level: `tests/api_axum.rs` (parity with actix, expected files in `tests/api_axum/`, regenerate with `ROK_SQLC_UPDATE_EXPECTED=1`, rustfmt).
- `check-arch` passes on axum projects today only because rule 1 names `actix_web`; the scaffolded `AGENTS.md` still says `actix_web` under axum (the init CLI test exempts `AGENTS.md`). Make `rules()`/`rules_markdown()` framework-aware.
- Wire messages: status and `code` match actix; the `400` message text for a bad path/query is axum's rejection text, not actix's.

**For ticket 03.** Copy the skeleton's axum files as the template's starting point; the `[api] openapi = false` guide edits (removing the Swagger lines from `app.rs`) are only written for actix.
