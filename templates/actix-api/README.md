# actix-api

A complete actix-web API made by [rok-sqlc](https://github.com/ateeq1999/rok-sqlc)
from one schema: the `users` and `audit_log` part of the axum starter's
OpenAPI document (`tests/spec/openapi.json`), with the same wire standard,
routes and schemas. `tests/contract.rs` checks the document this application
serves against that one, operation by operation.

Start one with:

```sh
rok-sqlc init --template actix my-api
cd my-api
rok-sqlc g
```

## Running it

You need PostgreSQL and Rust 1.94 or newer.

1. Create a database and apply `migrations/` to it, for example with
   `sqlx migrate run` ([sqlx-cli](https://crates.io/crates/sqlx-cli)) or
   `psql -f migrations/0001_init.sql`.
2. Set `DATABASE_URL` and `JWT_SECRET` (`.env.example` lists every
   setting; `src/config.rs` reads them from the environment).
3. `cargo run`. The API is under `/api/v1`, health at `/health/live` and
   `/health/ready`, the OpenAPI document at `/api-docs/openapi.json` and
   Swagger UI at `/docs/`.

Every route but health needs `Authorization: Bearer <token>`: an HS256 JWT
signed with `JWT_SECRET`, with `sub` (the user's id), `role` (`user` or
`admin`) and `exp`. `src/shared/auth.rs` verifies it. Issuing tokens (sign
in) is not part of the template yet; see below.

## Dependencies

The template's `Cargo.toml` has them. Starting from a package of your own
(`init --template` keeps an existing `Cargo.toml`), add these:

```toml
[dependencies]
actix-web = "4"
chrono = { version = "0.4", features = ["serde"] }
jsonwebtoken = { version = "11", features = ["rust_crypto"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sqlx = { version = "0.9", features = ["runtime-tokio", "postgres", "derive", "uuid", "chrono", "json"] }
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
utoipa = { version = "5", features = ["chrono", "uuid"] }
utoipa-swagger-ui = { version = "9", features = ["actix-web", "vendored"] }
uuid = { version = "1", features = ["v7", "serde"] }
validator = { version = "0.21", features = ["derive"] }

[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
todo = "deny"
unimplemented = "deny"
dbg_macro = "deny"
print_stdout = "deny"
```

## Checks

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
DATABASE_URL=postgres://... cargo test   # each test makes and drops its own schema
rok-sqlc g --check                       # the generated code matches the schema
rok-sqlc check-arch                      # the layout and rules of AGENTS.md
```

Without `DATABASE_URL`, the API tests print a note and pass; the contract
test needs no database. The lints in `Cargo.toml` deny `unwrap`, `expect`,
`panic`, `todo` and leftover debugging.

## What is generated, and what is yours

`AGENTS.md` and `src/features/README.md` explain the layout. In short:

- **Generated** (first line `@generated`; `rok-sqlc g` rewrites them, never
  edit them): `src/db/` (the data layer), `src/shared/{health,pagination,
  response,tenant,validation}.rs`, and every file of `src/features/` but
  `mod.rs` and each feature's `service.rs`.
- **Yours** (first line `Scaffolded once`, or no header): `rok-sqlc.toml`,
  `migrations/`, `src/{lib,main,app,config,error,openapi}.rs`,
  `src/shared/{mod,auth}.rs`, `src/features/mod.rs`, each feature's
  `service.rs` (business rules go there), `tests/`, and this README.

Change the schema with a new migration, adjust `rok-sqlc.toml`, run
`rok-sqlc g`, and the features follow.

## How the schema meets the target

- `role` is a PostgreSQL enum, so the schema `Role` is `user` or `admin`.
- `has_password` is a generated column (`password_hash IS NOT NULL`).
- `password_hash`, `totp_secret`, `avatar_key`, `failed_logins`,
  `locked_until`, `updated_at` and `deleted_at` are hidden
  (`[api.hidden]`): no response carries them.
- The columns the server sets are managed (`[conventions]
  managed_columns`), so no request body takes them; a trigger keeps
  `updated_at` current.
- `deleted_at` is the soft-delete column: `DELETE /users/{id}` marks the row.
- `email` is immutable (`[api.immutable]`): set when an account is created,
  never by `PATCH /users/{id}`, which answers `422` when its body names it.
  An email changes through the verification flow to come.
- `audit_log` is generated as the `audit_log_entry` module
  (`AuditLogEntryResponse`), served at `/audit-log`, read-only.
- `[api.search]` and `[api.sort]` give the users list its `q` and
  `UserSort` (`created_at`, `email`).
- `media` and `api_keys` are in the schema for the features to come and
  have no API yet (`[api] exclude`).

`tests/contract.rs` lists the differences that remain, each with its
reason: `avatar_url` (uploads), and the `?permanent=` and list `sort`/`order`
parameters every generated API has.

## Coming next

A later rok-sqlc release ports the rest of the axum starter as hand-written
features on top of this one:

- authentication: register, sign in and JWT issuing, two-factor (TOTP and
  recovery codes), password reset and change, email verification and
  change, invitations, OAuth, passkeys and QR sign-in;
- `POST /users` with a password, `/users/me`, and the business rules of
  `update` and `remove` (not yourself, not the last admin);
- uploads: avatars (`avatar_url`) and `media`;
- `api-keys` (create returning the key once, revoke, an unpaginated list);
- audit-log writes from those features.
