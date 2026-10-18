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

**Status:** ready-for-agent

- [ ] The snapshot and structural tests pass, and a deliberate annotation change fails the snapshot test.
- [ ] The README and guide drift tests pass, and the full suite passes, including PostgreSQL.
