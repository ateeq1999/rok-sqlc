# 0.7: axum decisions (A1–A8)

Settled with the maintainer on 2026-10-20. This note is the authority for `../spec.md`. Builds on ADR 0016 (generated APIs), ADR 0017 (pre-checks) and ADR 0018 (auth in the template).

- **A1: Picking the framework.** `[api] framework = "actix" | "axum"`, default `actix`, so existing projects are unchanged. `rok-sqlc init --framework axum` scaffolds the axum skeleton, and `init --template axum` copies the axum template. A project uses one framework.
- **A2: What the framework changes.** Only `routes.rs`, `handlers.rs`, the extractors (`AuthUser`, `AdminUser`, `Tenant`, `ValidatedJson`), the error-to-response mapping, and the server setup in `main.rs`/`app.rs`.
  - Identical across frameworks: `dto.rs`, `model.rs`, `repo.rs`, `service_api.rs`, `service.rs` (ports included), the data layer and the OpenAPI annotations.
  - axum uses `State`/`Path`/`Query`, `FromRequestParts` extractors and `AppError: IntoResponse`. Wire answers are identical.
- **A3: How handlers reach services.**
  - The generator emits a `Services` struct (one `Arc<XService>` per feature), built by the existing `register_services`, plus `impl FromRef<Services> for Arc<XService>`.
  - The scaffolded, user-owned `AppState` holds `services: Services` and the auth state, and derives `FromRef`.
  - Generated handlers take `State(service): State<Arc<UsersService>>`; generated routers are `Router<AppState>`.
- **A4: Middleware.** `tower-http` for tracing, CORS, request IDs, timeouts and body limits, mirroring the actix skeleton. The MIT `governor` limiter is ported as a tower `Layer`, with the same `429`/`Retry-After` behaviour (no `tower_governor`).
- **A5: OpenAPI.** `#[utoipa::path]` and the derived `OpenApi`, as in actix. Swagger UI comes from `utoipa-swagger-ui`'s `axum` feature. No `utoipa-axum` router.
- **A6: The axum template.** `templates/axum-api/`, generated from the same `schema.sql`, with the same contract test against the same OpenAPI document, and all of M5's auth. Services, mail, security, queries and migrations are the same code; only handlers, routes and extractors are rewritten.
  - Each template stays self-contained.
  - A repository test fails if a framework-independent file differs between them (`service.rs`, `dto.rs`, `shared/security`, `shared/mail`, `queries/`, `migrations/`).
- **A7: Generator tests.** A second fixture app, `tests/api-app-axum`, from the same schemas as `tests/api-app`. Its PostgreSQL suite mirrors actix's and asserts the same wire answers. `check-arch` knows the framework: handlers may name `axum`, not `actix_web`, and the reverse.
- **A8: Versions and release.** axum 0.8, tower-http 0.6, tokio (`rt-multi-thread`, `macros`, `signal`, `net`). Graceful shutdown uses `axum::serve(...).with_graceful_shutdown`, and the mail sender runs under `tokio::spawn`, stopping on the same signal. Ships as **0.7.0**. Workflow: integration branch `rok-sqlc-0-7`, tickets, review, PR, and the maintainer pushes the tag.
