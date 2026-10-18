# 02: The actix template and its contract test

**What to build:** `templates/actix-api/`, a complete project per the spec's Q46 and Q48.

**Schema.** `migrations/` builds on `../../rok-sqlc-0-4/notes/derived-schema.sql`, shaped so generation reproduces the covered target operations:
- `users`:
  - a `role` enum type (`Role`, labels `user`/`admin`);
  - `has_password BOOLEAN GENERATED ALWAYS AS (password_hash IS NOT NULL) STORED`;
  - the target's other `UserResponse` fields;
  - `password_hash`, `updated_at` and `deleted_at` hidden;
  - admin access;
  - search and sort columns as the target documents (`q`, a `UserSort` of `created_at`/`email`);
  - soft delete, if the target's DELETE is a soft delete (`openapi-standard.md` says it is);
- `audit_log`, renamed by module to `audit_log_entry`, at route `/audit-log`, read-only (skip `create`, `update` and `remove` per the target);
- keys are `UUID` with `primary_key_ids = "uuid_v7"`;
- the template may also contain the tables milestone 5 will need (media, api_keys, sessions), excluded with `api.exclude` for now, so its schema stays the real one.

**Configuration.** `rok-sqlc.toml` with `[api]` (`prefix` `/api/v1`, `access`, `hidden`, `search`, `sort`, `skip`, `routes`, `exclude`), `[modules]`, and conventions (`rok` preset or explicit, so `updated_at` is managed).

**Files:**
- the scaffolded skeleton (from `init`);
- the generated code, checked in (CI regenerates and fails on drift);
- a README: what it is, how to run it, what's generated vs yours, and what's coming in milestone 5;
- `Cargo.toml` with the exact dependency set, clippy lints matching the starters' strictness (`unwrap_used`, `expect_used`, `panic` and others deny);
- `.env.example`.

**Tests:**
- `tests/contract.rs`: the Q44 subset check. It loads the served document (via `actix_web::test`, no network) and the checked-in target copy `tests/target/openapi.json`. For each covered operation it compares method, path, operation id, tag, parameters, request body and success response structurally, resolving `$ref`s by name. It fails with a readable list of differences. Dangling refs in the target (`UserSort` and `SortOrder` are undefined there) are compared by name only. Keep the comparison logic in the template (self-contained), not in rok-sqlc.
- API tests against PostgreSQL for each covered route: happy path plus one failure, per the guidelines' testing section.

**Harness.** Wire the template into rok-sqlc's tests (`tests/fixture_compile/`):
- regenerate and check for no drift;
- run the lint gate, `check-arch`, and `cargo test` (with the DB URL);
- CI covers it.

Fix in the generator (not the template) any gap the contract test exposes that ADR 0016 says generation should cover, and record each in the Comments. Anything that needs milestone 5 is listed, not faked.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] The contract test passes for every covered operation, and a deliberate mismatch fails it with a clear message.
- [ ] The template builds clippy-clean, passes `check-arch` and its API tests against PostgreSQL (Debian, as CI), and shows no generation drift.
