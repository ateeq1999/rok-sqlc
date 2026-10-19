# 03: The rest of the guide

**What to build:** `docs/guide/index.md` lists pages "coming". Write them, with every code sample copied from a compiled source (`docs/examples/api`, or `templates/actix-api` once ticket 02 lands; the drift tests in `tests/readme.rs` already cover `docs/guide/*.md`):

- **getting-started.md:** from `rok-sqlc init` (and `init --template actix`) to a running API. Covers migrations, `rok-sqlc g`, `.env`, `cargo run`, the first requests, and Swagger UI at `/docs/`.
- **configuring.md:** every `[api]` key, with an example of each and what it changes in the routes and the document. That's access, owner, hidden, search, sort, skip, routes, exclude, prefix, `search_endpoint` and `openapi`. Also the conventions that matter for APIs (managed columns, soft delete, tenant, version) and `primary_key_ids` (`uuid_v7`, `cuid2`, …).
- **customizing.md:** overriding a service method (`_in` vs the transaction wrapper), adding state to a service (`XService::new`), writing a port and its adapter in `app.rs`, and a hand-written feature with its own `openapi.rs` paths.
- **wire-standard.md:** every status, error code and body the generated API answers, with examples, plus the list shape and headers (`Location`, `ETag`, `If-Match`).
- **testing.md:** API tests with `actix_web::test` against a database, and service tests.
- **deploying.md:** configuration from env, pool settings, `/health/live` versus `/health/ready`, running migrations as a separate step, and `check-arch` in CI.
- **after-a-schema-change.md:** what regenerates, what stays yours, what breaks and how to fix it, and `--check` in CI.
- **troubleshooting.md:** each config, layout and `check-arch` error with its fix.

Update `index.md` (no "coming" left for these), and link the guide from the README.

**Blocked by:** 01

**Status:** resolved

- [x] Every page exists with samples from compiled sources; drift tests pass.
- [x] Each page's claims match the code. Spot-check every error message quoted in the troubleshooting page against the source.

## Comments

Done on branch `rok-sqlc-0-4-m3-03`.

- **Pages:** `docs/guide/{getting-started,configuring,customizing,wire-standard,testing,deploying,after-a-schema-change,troubleshooting}.md`. `index.md` lists every page (no "Coming" section); the README's "Generated APIs" section links them all; `architecture.md` links testing and the hand-written feature; CHANGELOG `[Unreleased]` entry.
- **Example extended** (`docs/examples/api`): a hand-written `stats` feature (`GET /api/v1/stats`, `ProjectStats` query in `queries.sql`, `#[utoipa::path]` listed in `openapi.rs`'s `ApiDoc` with its tag, mounted in `app.rs`, declared in `features/mod.rs`); `tests/api.rs` (`actix_web::test` over `app::configure`), `tests/service.rs`, `tests/support/mod.rs`, all `#[sqlx::test]` (sqlx's default `macros`/`migrate` features; no new dependencies, lock unchanged).
- **Tests:** `tests/readme.rs` requires all 12 guide pages, and the new `the_api_example_tests_pass` runs the example's tests when `ROK_SQLC_TEST_DATABASE_URL` is set (as `DATABASE_URL`; sqlx creates a database per test).
- **Verification:** every quoted error was produced by the built binary on scratch projects (config, generation, `--check`, `init --template`, every `check-arch` rule) or by the example binary (start-up errors); every HTTP exchange is a real answer of the example server run against PostgreSQL.
- **Changed samples:** `openapi.md` and the README now source the scaffolded `ApiDoc` and `configure` from `src/init/templates/` (the example's are no longer the untouched scaffold); `layout.md`'s tree shows `stats/`.
- **Template:** pages link `templates/actix-api/` and its `README.md` by path (ticket 02 writes them) and describe `init --template actix` without quoting its next-steps text, which ticket 01 prints with a stray 19-space indent (`src/cli.rs`, template arm) — worth fixing in 02 or 04.
- **Noticed:** the scaffolded `main.rs` installs no `tracing` subscriber, so the logs `deploying.md` mentions go nowhere until the user adds one (documented, not changed). A start-up config error prints as `Error: Custom { kind: Other, error: Missing("JWT_SECRET") }` (the `Debug` of `io::Error`); a friendlier `main.rs` would be a scaffold change.
