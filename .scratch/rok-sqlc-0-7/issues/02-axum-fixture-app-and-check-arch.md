# 02: `tests/api-app-axum` wire parity, and `check-arch` per framework

**What to build:** decision A7.

- **`tests/api-app-axum`:**
  - Generated with `framework = "axum"` from the same migrations and config as `tests/api-app` (share them rather than copy where the harness allows).
  - Port `tests/api-app`'s PostgreSQL suites (shapes, references, unique, rules, relations, owner/tenant scoping, versioning/ETag, pagination, errors) so they assert the same statuses and bodies.
  - Factor shared assertions to keep the two suites from drifting, for example a shared test module driven over an HTTP client against either app, if that's practical. Otherwise mirror file by file and say so.
  - Wire it into `tests/fixture_compile` like `api_app.rs`.
- **`check-arch`:** detect the project's framework from `rok-sqlc.toml`. Under axum, the handler and route rules accept `axum::…` and forbid `actix_web::…`; under actix, the reverse. Every other rule stays the same. Add rule tests for both.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] The axum fixture suite passes against PostgreSQL with the same answers as actix.
- [ ] `check-arch` rule tests for both frameworks.
- [ ] Gates pass.
