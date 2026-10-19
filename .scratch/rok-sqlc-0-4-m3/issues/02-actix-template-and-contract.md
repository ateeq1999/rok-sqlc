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

**Status:** resolved

- [x] The contract test passes for every covered operation, and a deliberate mismatch fails it with a clear message.
- [x] The template builds clippy-clean, passes `check-arch` and its API tests against PostgreSQL (Debian, as CI), and shows no generation drift.

## Comments

Done on branch `rok-sqlc-0-4-m3-02`.

**The template** (`templates/actix-api/`):
- `Cargo.toml.template`: crate `actix-api`, Rust 1.94, the dependency set, `unsafe_code` forbidden, and clippy `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, `dbg_macro` and `print_stdout` denied.
- `Cargo.lock`: `init` skips it (its first line has `@generated`), and the harness copies it in.
- `rok-sqlc.toml` and `migrations/0001_init.sql`.
- The skeleton `init` scaffolds, unchanged.
- The generated code, checked in, and the scaffolded `service.rs` files.
- `README.md`: running it, its dependencies, generated vs yours, how the schema meets the target, and what comes next.
- `.env.example` and `.gitignore`.
- `tests/`: `contract.rs`, `users.rs`, `audit_log.rs`, `health.rs`, `common/mod.rs`, and `target/openapi.json` (a copy of `.scratch/rok-sqlc-0-4/notes/openapi.json`).

The schema:
- a `role` enum;
- `has_password BOOLEAN NOT NULL GENERATED ALWAYS AS (password_hash IS NOT NULL) STORED`. I added `NOT NULL` so the response field is a required `boolean`, as in the target;
- `users_email_key` on `lower(email)` for live rows;
- a `touch_updated_at` trigger;
- `audit_log.details JSONB NOT NULL DEFAULT '{}'`. The target's `details` is required and untyped, and a `NOT NULL` `serde_json::Value` gives exactly that;
- `media` and `api_keys`, present but in `api.exclude`;
- UUID keys with `primary_key_ids = "uuid_v7"`.

The config:
- `soft_delete_column = "deleted_at"`;
- managed columns: `created_at`, `updated_at`, `password_hash`, `email_verified`, `totp_secret`, `totp_enabled`, `avatar_key`, `failed_logins` and `locked_until`;
- `[modules] audit_log = "audit_log_entry"`, with the route `/audit-log`;
- both tables admin only;
- users skip `create`, `restore`, `lookups` and `relationships`;
- audit_log skips `create`, `update`, `remove` and `relationships`. It keeps `get_one`, an extra operation;
- hidden, search (`email`, `display_name`) and sort (`created_at`, `email`) columns as the target has them.

**The contract** (`tests/contract.rs`, self-contained, needs only serde_json):
- It covers the users list, get, update and remove, the audit-log list, `/health/live` and `/health/ready`.
- For each operation it compares the operation id and tags, the parameters (by name and `in`: required and schema), the request body, and the lowest 2xx status with its JSON schema.
- Schemas are compared recursively and structurally:
  - every keyword except the annotations (`description`, `summary`, `title`, `example(s)`, `default`, `deprecated`);
  - `type`, `required` and `enum` as sets;
  - `$ref`s resolved in each document, with their names compared;
  - a dangling target ref (`UserSort`, `SortOrder`) by name only.
- When a target response has no body, any served body is accepted.
- The policy is otherwise strict: anything else that differs fails, including extra params and extra body or response properties.

`KNOWN` lists the differences that remain, each with its reason. The test also fails when a known difference goes away. The entries:
1. `UserResponse.avatar_url` is missing from the list, get and update responses. Needs milestone 5 (uploads).
2. `UpdateUserDto` has `email`. The target changes an email through its auth flow (milestone 5). Generation has no way to hide a column from bodies only. `managed_columns` would break `create`, because `email` is `NOT NULL` with no default, and ADR 0016 doesn't call for such a mechanism.
3. `DELETE /users/{id}` takes `permanent`.
4. `GET /audit-log` takes `sort`.
5. `GET /audit-log` takes `order`.

Entries 3 to 5 aren't gaps: ADR 0016 has every soft-delete `DELETE` take `?permanent=` and every list take `sort` and `order`.

`a_mismatch_breaks_the_contract_with_a_readable_list` changes a type and an operation id and drops a parameter, then checks the exact lines reported.

**Harness** (`tests/fixture_compile/actix_template.rs`, so CI's `cargo test` runs it against its PostgreSQL). It runs `init --template actix` into a temp dir, then `g`, and copies in the template's `Cargo.lock`. Then it checks:
1. The `@generated` files equal the template's checked-in ones. `ROK_SQLC_UPDATE_EXPECTED=1` rewrites them and deletes stale ones.
2. The README's dependency block matches `Cargo.toml.template`.
3. `check-arch`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` (no warning), and `cargo test` with `DATABASE_URL` set to `ROK_SQLC_TEST_DATABASE_URL`. Without that URL the API tests skip with a note; the contract test needs no database.

The build is skipped on toolchains below 1.94. The root `.gitignore` ignores `templates/actix-api/Cargo.toml` and `target/`.

**Generator fixes:** the contract exposed no gap that ADR 0016 says generation should cover. I made one CLI fix in `init --template`'s next steps:
- They printed with stray indentation (a raw multi-line string).
- They told the user to add the dependencies from `rok-sqlc.toml`, which the template's `rok-sqlc.toml` doesn't list.

They are now numbered lines, and the dependencies step appears only when the project kept its own `Cargo.toml`, pointing at the list in the template's README. Tests are in `tests/cli/init_template.rs`.

**Milestone 5** (also listed in the template's README):
- the auth surface: register, sign in and token issuing, 2FA/TOTP and recovery codes, password reset and change, email verification and change, invitations, OAuth, passkeys and QR sign-in;
- `POST /users` with `password` (users `create` is skipped until then);
- `/users/me`;
- `avatar_url` and avatars;
- `media` (binary upload, `metadata`);
- `api-keys` (`CreatedApiKeyResponse`, `revoke`, a bare-array list);
- the users business rules (not yourself, not the last admin), which are the target's 400s on update and remove;
- audit-log writes.

Once those land, `KNOWN` entries 1 and 2 should be removed.

**Deviations:**
- The API tests make a schema per test from `db::schema::SCHEMA_SQL` (as `tests/api-app` does) rather than using `#[sqlx::test]`, so they skip without `DATABASE_URL`.
- An unknown enum value in a body is a 422 `validation_failed` (the generated extractor's choice), not a 400.
