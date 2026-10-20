# 04: M5 auth in the axum template

**What to build:** decision A6, for the M5 features (see `../rok-sqlc-0-6/spec.md` and ADR 0018).

- Port `features/auth/` and `features/account/` to `templates/axum-api/`.
  - `dto.rs` and `service.rs` (with the `AuditTrail` port) are byte-identical to actix's.
  - `routes.rs`/`handlers.rs` are rewritten for axum, with the rate-limit layer on the `/auth` routes and `/auth/me`, and `account` mounted before the generated `users` routes so `/users/me` isn't captured by `/users/{id}` (check axum's router semantics: static segments win over captures there, so say what was needed).
- `AuthUser`/`AdminUser` load the session state per request (Q21), as `FromRequestParts` over `AppState`.
- The `AuditLog` port implementation in `app.rs`.
- Port every PostgreSQL flow test (`sign_up`, `login`, `account`, `password`, `email_change`, `invitations`, `auth`, `mail`) with the same assertions.
- The contract test covers all 13 M5 paths.
- Extend the drift test to `features/auth/{dto,service}.rs`, `features/account/{dto,service}.rs` and the `audit_log_entry/service.rs` `create_in`.

**Blocked by:** 03

**Status:** resolved

- [x] Every ported flow test passes on PostgreSQL. The contract test covers the M5 paths.
- [x] The drift test covers the auth files.
- [x] Gates pass.

## Comments

Done on `rok-sqlc-0-7-04`.

- **Byte-identical files.** `features/{auth,account}/{dto,service}.rs` are actix's copies; nothing in them was actix-specific. One edit to both templates: the services' doc line "registered as app data by `app.rs`" now reads "built by `app.rs`". `audit_log_entry/service.rs` (with `create_in`) was already identical. The drift test checks `auth` and `account` through a new `HAND_WRITTEN` list with its own `HAND_WRITTEN_FILES` (`dto.rs`, `service.rs`), not through `FEATURES`. The test fails on a listed path that neither template has, and these features have no `model.rs`, `repo.rs` or `service_api.rs`.
- **Services.** `app.rs` implements `FromRef<AppState>` for `Arc<AuthService>` and `Arc<AccountService>`, built from the state on each request that takes them (a few `Arc` clones). That way a test that changes `state.breach` or `state.audit` after `AppState::new` sees its change, as actix's `configure` gives. `AppState` gained `audit: Arc<dyn AuditTrail>` (the `AuditLog` adapter, ported as is).
- **Routing.** `auth::routes(&RateLimit)` and `account::routes(&RateLimit)` return `Router<AppState>`. `/api/v1/auth/me` is its own small router with `.layer(limit)`, merged with `/api/v1/users/me` (not limited, as in actix). `app::router` merges `account` and `auth` before `routes()`. The order doesn't matter: matchit prefers the static `/users/me` to `/users/{id}`, and merging two routers with distinct paths doesn't conflict. So nothing special was needed.
- **Handlers.** The same `#[utoipa::path]` blocks. `State<Arc<..Service>>` and `(StatusCode, Json(..)).into_response()`. `openapi.rs` is now identical to actix's.
- **Tests.** `sign_up`, `login`, `account`, `password`, `email_change` and `invitations` are mechanical ports (`#[tokio::test]`, `axum_api`). `support` gains `PASSWORD`/`register_verified`. The contract test's `COVERED`, `KNOWN` and mismatch lines match actix's. `auth` and `mail` were ported in 03.
- README (an Accounts section and the files that are yours) and `AGENTS.md` (the `account`/`auth` line) are updated.
