# 05: Docs, review fixes, and 0.7.0

**What to build:**
- **ADR 0019:** axum support (A1–A8). It covers what differs per framework, `Services` + `FromRef`, why there's no `utoipa-axum`, the drift test between templates, and the rate-limit layer.
- **Guide:** the pages that show actix code (getting started, architecture, configuring, auth, wire standard, troubleshooting) gain axum where it differs (`[api] framework`, `init --framework axum`, `init --template axum`, handler and extractor examples). Keep the shared parts written once.
- **README:** templates section, configuration, `init` options, and the roadmap (axum done; M6 next for both templates).
- **0.7.0:**
  - bump the version everywhere (precedent: commit a191d97);
  - CHANGELOG `[0.7.0]` with breaking changes first, if any (for example, the generated root's shape changing for actix);
  - `cargo publish --dry-run --locked`.

Do NOT tag or publish.

**Blocked by:** 01, 02, 03, 04

**Status:** resolved

- [x] Drift tests, and the full suite on Rust 1.94 against Debian PostgreSQL.
- [x] The 1.88 build and the publish dry-run.

## Comments

Done on `rok-sqlc-0-7-05`. Review fixes first, each its own commit:

1. **408 body.** Both axum `app.rs` (skeleton and template) apply `app::time_out` (`middleware::from_fn_with_state(REQUEST_TIMEOUT, time_out)` over `tokio::time::timeout`), answering `AppError::RequestTimeout` (`408 request_timeout`, JSON body). tower-http's `timeout` feature stays in the hints (removing it changes `Cargo.lock`); the skeleton's tokio hint gains `time`. Tests: `templates/axum-api/tests/timeout.rs`, the init e2e `DOCS_TEST`, `axum_tests.rs`.
2. **Auth scope.** axum nests the auth routes under `/api/v1/auth` with `fallback` and `method_not_allowed_fallback` (both `404 not_found`) behind the limit. actix-web's resources inherit the scope default (wrong method = 404), so the actix scope got the same JSON `404` default service. Test in both templates' `login.rs`.
3/4. **Shared limiter + missing peer** (one commit, same file): `shared/rate_limit/limiter.rs` is byte-identical and in the drift test's `SHARED`; `client_ip` takes the header's `&str` values; `check` asks for the peer lazily. axum's `mod.rs` warns once (`std::sync::Once`) without `ConnectInfo`; unit-tested via `Once::is_completed`.
5. **Services once.** axum `AppState` gains `auth`/`account` fields; `with_breach`/`with_audit` use `Arc::make_mut` (so `AuthService` derives `Clone` in both templates). actix unchanged (built once per worker in `configure`).
6. **Seam.** `ApiFramework::{crate_name, other, wiring, swagger_ui_feature, skeleton}`; AGENTS.md now says "declares the features; `register_services` and `configure_all`" (the README wording). `relation_needs` holds the shared tenant/caller logic. `generate.rs` names `ApiFramework::Axum` (still 597 lines).
7. `SHARED_ITEMS` constant + unit test; output byte-identical.
8. Wiring-bug path rejections (`MissingPathParams`, wrong count, unsupported type) are `AppError::Internal`; invalid UTF-8 stays 400. Test in `tests/api-app-axum/tests/shared.rs`.
9. `Status::{Ok, Created}`; `Extracted::value()`.

Docs: ADR 0019; guide pages (index, getting started, configuring `framework`, architecture with axum's rule 1 quoted and tested, auth, wire standard "What the frameworks add", testing, troubleshooting with real outputs); README; CHANGELOG `[0.7.0]`; version 0.7.0 (Cargo.toml/lock, install scripts, release.yml, README).
