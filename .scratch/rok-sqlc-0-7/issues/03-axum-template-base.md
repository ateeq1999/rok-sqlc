# 03: `templates/axum-api/`: the base, without auth

**What to build:** decisions A4, A6 and A8, without the M5 auth features (ticket 04).

- **`templates/axum-api/`:**
  - Embedded in the binary like `actix-api` (via `build.rs`, `Cargo.toml.template`). `init --template axum` copies it.
  - Same `migrations/` (both), `rok-sqlc.toml` with `framework = "axum"`, the generated `users`/`audit_log_entry` features, and `tests/spec/openapi.json`.
  - A contract test equivalent to actix's (the same COVERED/KNOWN approach) for the generated CRUD paths and health.
- **Server (`main.rs`/`app.rs`):**
  - tower-http layers (A4); graceful shutdown;
  - the mail **sender** under `tokio::spawn`, stopped by the same shutdown signal (port `shared/mail` from actix; only the spawn/shutdown wiring differs);
  - the **rate limiter** ported as a tower `Layer` over `governor` with the same `429` + `Retry-After` and `TRUST_PROXY` rules.
- **Shared code:** the framework-independent files listed in A6 are byte-identical to `templates/actix-api`'s.
- **Drift test (A6):** a repository test that fails when any of those files differ between the two templates, naming the file. Today's list: `migrations/`, `queries/`, `src/shared/security/`, `src/shared/mail/` (except a file that owns spawn wiring, if any; say which), and every feature's `dto.rs`/`service.rs`/`model.rs`/`repo.rs`/`service_api.rs`. Ticket 04 extends it to `auth`/`account`.
- **CI:** build and test the axum template like the actix one (`tests/fixture_compile/axum_template.rs`).

**Blocked by:** 01. It can run in parallel with 02.

**Status:** resolved

- [x] The template builds, is clippy-clean on 1.94, passes `check-arch` and its contract test.
- [x] The PostgreSQL tests for the generated features, mail (sender start/stop) and the rate limit layer pass.
- [x] The drift test passes and fails on an injected difference (a unit test of the comparer).
- [x] Gates pass.

## Comments

Done on `rok-sqlc-0-7-03`.

**Decisions.**
- `queries/` is copied whole (`account.sql`, `auth.sql`, `mail.sql`, `session.sql`), so the drift test compares the directory as is; `db::queries::{auth,account}` are generated but unused until 04.
- Byte-identical `shared/security/` needed one change to the actix template: `password.rs` uses `tokio::task::spawn_blocking` instead of `actix_web::web::block`, and the security tests use `#[tokio::test]`. The actix `Cargo.toml.template` (and README block) gained tokio's `rt` feature; `Cargo.lock` unchanged.
- `shared/mail/` was already framework-free: all of it is shared. No file owns the spawn wiring; each `app.rs` does.
- `error.rs` already carries every actix variant (`InvalidToken`, `WrongPassword`, `BreachedPassword`, `TooManyRequests` with `Retry-After`), so 04 needn't touch it.
- `docs/guide/troubleshooting.md`: the unknown-template message now lists `actix, axum`.

**Layout (`templates/axum-api/`).** Same as actix minus `features/{auth,account}`, `tests/{sign_up,login,account,password,email_change,invitations}.rs` and `support::register_verified`/`PASSWORD`. Hand-written: `src/{app,main,lib,config,error,openapi}.rs`, `src/features/mod.rs` (`mod generated; pub use generated::*;`, declare `pub mod auth; pub mod account;` here), `src/shared/{mod,auth,rate_limit}.rs`, `shared/security/`, `shared/mail/`. `config.rs` = actix's with `MAIL_FROM` default `Axum API <...>`. `openapi.rs`'s `ApiDoc` has empty `paths()`: add the auth handlers there.

**`AppState`** (`#[derive(Clone, FromRef)]`, `AppState::new(pool, &Config)`): `pool: PgPool`, `services: Services`, `keys: Arc<JwtKeys>`, `outbox: Outbox`, `templates: Arc<Templates>`, `breach: Arc<dyn BreachCheck>`, `rate_limit: RateLimit`, `require_verified_email: bool` (`#[from_ref(skip)]`). 04 adds `audit: Arc<dyn AuditTrail>` (actix's `AuditLog` adapter in `app.rs`) and builds `AuthService`/`AccountService` (e.g. as `Arc<...>` fields, which `FromRef` then hands to handlers via `State<Arc<AuthService>>`).

**Routing.** `app::router(state) -> Router` merges `health::routes()`, Swagger UI, then `features::routes()`, then the tower-http layers, then `.with_state(state)`. Mount 04's routers by `.merge(account::routes(&state.rate_limit))` / `.merge(auth::routes(..))` before `routes()` (axum's matchit prefers the static `/users/me` over `/users/{id}` anyway). Rate-limit a router with `.layer(limit.clone())` (`RateLimit` is a tower `Layer`; the peer comes from `ConnectInfo<SocketAddr>`, which `app::serve` provides via `into_make_service_with_connect_info`; tests without it count as `0.0.0.0`). `app::serve(listener, state, Arc<dyn Mailer>, shutdown)` runs the server and the `tokio::spawn`ed sender, both ended by `shutdown`; `run` passes Ctrl+C/SIGTERM.

**Extractors.** `shared/auth.rs`: `AuthUser`/`AdminUser` need `Arc<JwtKeys>: FromRef<S>` and `PgPool: FromRef<S>`; per-request `db::queries::session::session_state` (Q21), same 401/403 rules and unit tests as actix.

**Test support (`tests/support/mod.rs`).** Same API as actix: `Database::fresh()`/`drop()`, `call(pool, token, Method, uri, body)`, `call_with(&AppState, ...)` (shares a rate limit; builds `router(state.clone())` and `oneshot`s it), `Answer { status, body }`, `keys() -> JwtKeys` (not `web::Data`), `token`, `account`, `admin`, `user`, `config(&[..])`, `state`, `state_with`, `mailed`, `mailed_token`, `APP_URL`. Port `PASSWORD` and `register_verified` from actix in 04. Tests: `auth.rs`, `users.rs`, `audit_log_entry.rs`, `health.rs`, `mail.rs` (ports), `rate_limit.rs` (the layer, no DB), `server.rs` (`serve` + sender start/stop on a real socket), `contract.rs` (`COVERED` = generated CRUD + health; add the auth operations and their `avatar_url` `KNOWN` lines, and the mismatch test's expected lines, from actix's).

**Drift test.** `tests/template_drift.rs`: `SHARED` (`migrations`, `queries`, `src/shared/security`, `src/shared/mail`), `FEATURES` (`users`, `audit_log_entry`) x `FEATURE_FILES` (`dto.rs`, `model.rs`, `repo.rs`, `service.rs`, `service_api.rs`; a file absent from both is fine, from one side is drift). CRLF is normalized. 04: add `"auth"`, `"account"` to `FEATURES`.

**Template tests.** `tests/fixture_compile/template_project.rs` holds the shared `Template { name, dir }` checks (`checks_in_what_generation_writes`, `readme_lists_its_manifest`, `passes_its_checks_and_tests`); `actix_template.rs` and `axum_template.rs` call them. Regenerate checked-in files with `ROK_SQLC_UPDATE_EXPECTED=1 cargo test --test fixture_compile template_checks_in`. `Cargo.lock` = actix's plus axum 0.8.9, axum-core, axum-macros, matchit 0.8.4, serde_path_to_error (same versions as `tests/api-app-axum`).

**check-arch** passes as is (rule 1 names `actix_web` today; the axum `AGENTS.md` already says `axum`, so ticket 02's framework-aware text may need it re-synced).
