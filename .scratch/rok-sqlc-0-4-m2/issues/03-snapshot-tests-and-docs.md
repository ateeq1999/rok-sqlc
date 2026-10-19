# 03: The document's snapshot test, structural checks, and docs

**What to build:**

**Snapshot test.** `tests/api-app/tests/openapi.rs` compares the served `/api-docs/openapi.json` with a checked-in snapshot, `tests/api-app/tests/snapshots/openapi.json`. Keys are sorted so the comparison is stable. There's an environment switch to rewrite the snapshot, documented in the test file.

**Structural checks** run over every operation in the document, against the wire standard:
- every path is under `api.prefix`;
- every `4xx`/`5xx` response refers to `ErrorBody`;
- lists answer `PaginatedResponse`, `POST` creates answer `201`, and deletes answer `204`;
- routes with access `user` or `admin` carry `bearer_auth`, and public ones don't;
- operation ids are handler names, and tags are lowercase kebab-case.

**`[api] openapi = false`** is a config key (default `true`), with README `config-reference` and drift tests. Turning it off gives exactly milestone 1's output; test that.

**Docs:**
- the README "Generated APIs" section: the document, Swagger UI, and adding hand-written paths to `src/openapi.rs`;
- `docs/guide/` gains an `openapi.md` page, with samples from `docs/examples/api`, which is regenerated and serves the docs;
- the CHANGELOG `[Unreleased]` entry;
- ADR 0016's OpenAPI line, updated to what was built.

**Blocked by:** 02

**Status:** resolved

- [x] The snapshot and structural tests pass, and a deliberate annotation change fails the snapshot test.
- [x] The README and guide drift tests pass, and the full suite passes, including PostgreSQL.

## Comments

Resolved on branch `rok-sqlc-0-4-m2-03`.

What was built:

- `tests/api-app/tests/openapi.rs`: the snapshot test (`tests/api-app/tests/snapshots/openapi.json`, keys sorted, `ROK_SQLC_UPDATE_SNAPSHOTS=1` rewrites it) and the structural checks: paths under `/api/v1/` (health under `/health/`), every non-2xx response is `ErrorBody`, `list`/`search` answer `PaginatedResponse_*`, `create` `201`, every `DELETE` `204`, `bearer_auth` exactly on routes needing a caller (from the fixture's access; a relationship route takes the stricter side; a public route may carry the optional `[{}, {"bearer_auth": []}]` of a stricter `?include=`), operation ids are snake_case and name a `pub async fn` in a `handlers.rs` or `health.rs`, tags kebab-case and declared. The health rename below failed the snapshot test before the snapshot was rewritten. `fetch`/`document` moved to `tests/common/mod.rs`.
- `[api] openapi = false` is exactly milestone 1's output: `tests/api_openapi.rs::without_openapi_the_output_is_milestone_1s` runs the binary on `tests/openapi_off/rok-sqlc.toml` (api-app's migrations and settings) and compares per-file FNV-1a digests with `tests/openapi_off/milestone-1.txt`. I made the record with milestone 1's binary (commit a685022) and checked it: the only differing files are `db/pool.rs` (doc-comment fixes from the PR #6 CI fixes) and `shared/health.rs` (the rename below). Both changes were intended.
- Follow-up 1: the health handlers are now `liveness`/`readiness`, so operation ids still match handler names. Their descriptions are the target's. `/health/ready`'s `503` keeps `body = ErrorBody` (deviation: the target document has no error schemas, but our standard requires `ErrorBody` on every error status). The handlers were renamed rather than given an `operation_id`, so output without OpenAPI changes too.
- Follow-up 2: `utoipa-swagger-ui` has `vendored` in 9.0.2 (`utoipa-swagger-ui-vendored` 0.1.2, checked in the registry). The hint in `db/mod.rs` and `init`'s `rok-sqlc.toml` is now `features = ["actix-web", "vendored"]`, and so are `tests/api-app` (lock: + `utoipa-swagger-ui-vendored` only) and `docs/examples/api`.
- `docs/examples/api` was regenerated, and it now serves the docs: I added `src/openapi.rs`, `pub mod openapi;`, the `app.rs` routes, the dependency and the hint line by hand, as the guide says to. Its lock only gained the new packages.
- Docs: the README "Generated APIs" section, the `init` table (`app.rs`, `src/openapi.rs`) and the `[api]` prose; a new `docs/guide/openapi.md` (all its samples are sourced from `docs/examples/api`), added to `GUIDE_PAGES` and the index and dropped from "Coming"; `layout.md`; the CHANGELOG `[Unreleased]` entry; ADR 0016's OpenAPI line.

For the milestone review / milestone 3:

- `src/generator/models.rs` is 826 lines, over the ~600-line limit. Not split here.
- utoipa fills operations' `summary`/`description` from the handlers' doc comments. The wire standard says the target has no `summary` fields, so milestone 3's contract test will need to ignore them or strip them.
