# rok-sqlc 0.4, milestone 3: the actix template, its contract test, the guide, and 0.4.0

## Problem

Milestones 1 and 2 generate a full actix-web API with an OpenAPI document. There's no ready-made starting point that shows the result end to end, nothing proves the result meets the target spec (`../rok-sqlc-0-4/notes/openapi.json`, the axum starter's document), and the guide is three pages short of what users need.

## Decisions

Settled with the maintainer on 2026-10-19 (questions Q44 to Q49); ADR 0016 is the authority. Inputs: `../rok-sqlc-0-4-m2/notes/target-differences.md`, `../rok-sqlc-0-4/notes/{openapi-standard.md,derived-schema.sql,starters.md}`.

- **Q44, a subset contract:** for every operation of the target spec that milestone 3 covers, the generated document must match it:
  - the same method, path, operation id and tag;
  - the same parameters (name, `in`, required, schema type and format);
  - the same request body schema;
  - the same success status and response schema, compared structurally (properties, types, formats, nullability, required, enum values, `$ref` targets by name).

  Extra statuses, error bodies, summaries and descriptions, and extra operations are allowed.
- **Q45, the coverage:** the contract covers these target operations: `users` list, get, update and remove (the admin CRUD); `audit-log` list; and `/health/live` and `/health/ready`.

  Milestone 5 covers the rest, along with the auth endpoints:
  - `media` (binary upload, `metadata`);
  - `api-keys` (`CreatedApiKeyResponse`, `revoke`, a bare-array list);
  - the users fields that depend on uploads (`avatar_url`) and on the password flow (the `password` input);
  - `/users/me`.
- **Q46, closing differences in the template's SQL and config:**
  - a `role` enum type makes `Role`;
  - `has_password BOOLEAN GENERATED ALWAYS AS (password_hash IS NOT NULL) STORED`;
  - module `audit_log` is renamed `audit_log_entry` (route kept as `/audit-log`), making `AuditLogEntryResponse`;
  - hidden `updated_at`, `deleted_at` and `password_hash`;
  - managed and server-set columns through conventions.
- **Q47, generator alignments:** `PaginatedResponse.total` gets `minimum: 0`. Path parameter descriptions read like the target ("User id": the singular table name and the key column).
- **Q48, the template:** `templates/actix-api/` holds a `rok-sqlc.toml`, `migrations/` (the derived schema), the scaffolded skeleton, the generated code (checked in, drift-checked by CI), a README, `tests/contract.rs` (the Q44 check against a checked-in `tests/target/openapi.json`), and API tests against PostgreSQL.

  `rok-sqlc init --template actix` copies the template's user-owned and config files into a project and then runs no generation. It prints the next steps instead. The template's files are embedded in the binary, so `init` works from `cargo install`.
- **Q49, the release:** prepare 0.4.0 (version, CHANGELOG `[0.4.0]`, publish dry run) and stop. The maintainer pushes `v0.4.0`, and the Release workflow builds the binaries, creates the GitHub release, and publishes to crates.io.

## Constraints

These are unchanged:
- the tool builds on Rust 1.88;
- generated code needs Rust 1.94, passes the lint gates, `check-arch` and rustfmt, and adds no unwrap, expect or panic;
- fresh `init` projects, and the template, build clippy-clean;
- output without `[api]` is unchanged;
- CI runs PostgreSQL 17 on Debian (glibc collation).

## Tickets

`issues/01` to `issues/04`: 01 first; then 02 and 03 in parallel; then 04.
