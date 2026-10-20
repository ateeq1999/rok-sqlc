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

**Status:** ready-for-agent

- [ ] The template builds, is clippy-clean on 1.94, passes `check-arch` and its contract test.
- [ ] The PostgreSQL tests for the generated features, mail (sender start/stop) and the rate limit layer pass.
- [ ] The drift test passes and fails on an injected difference (a unit test of the comparer).
- [ ] Gates pass.
