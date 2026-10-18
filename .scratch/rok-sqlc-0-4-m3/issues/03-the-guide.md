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

**Status:** ready-for-agent

- [ ] Every page exists with samples from compiled sources; drift tests pass.
- [ ] Each page's claims match the code. Spot-check every error message quoted in the troubleshooting page against the source.
