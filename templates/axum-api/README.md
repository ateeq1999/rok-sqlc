# axum-api

A complete axum API made by [rok-sqlc](https://github.com/ateeq1999/rok-sqlc)
from one schema: the `users` and `audit_log` part of the axum starter's
OpenAPI document (`tests/spec/openapi.json`), with the same wire standard,
routes and schemas. `tests/contract.rs` checks the document this application
serves against that one, operation by operation. It is the actix template
(`rok-sqlc init --template actix`) on axum 0.8: the same schema, queries,
services, security and mail code, with axum handlers, routes and
extractors.

Start one with:

```sh
rok-sqlc init --template axum my-api
cd my-api
rok-sqlc g
```

## Running it

You need PostgreSQL and Rust 1.94 or newer.

1. Create a database and apply `migrations/` to it, for example with
   `sqlx migrate run` ([sqlx-cli](https://crates.io/crates/sqlx-cli)) or
   `psql` on each file of `migrations/` in order.
2. Set `DATABASE_URL` and `JWT_SECRET`, at least 32 bytes (`openssl rand
   -base64 48`). `.env.example` lists every setting; `src/config.rs` reads
   them from the environment.
3. `cargo run`. The API is under `/api/v1`, health at `/health/live` and
   `/health/ready`, the OpenAPI document at `/api-docs/openapi.json` and
   Swagger UI at `/docs/`. Ctrl+C or `SIGTERM` stops it gracefully:
   requests in flight finish, then the mail sender stops.

## Accounts

The account flows are hand-written features, yours to change:
`src/features/auth/` and `src/features/account/`. rok-sqlc's guide
documents each one, the settings, sending real mail and adding a flow
([Accounts and sign-in](https://github.com/ateeq1999/rok-sqlc/blob/main/docs/guide/auth.md)).

- **Sign-up.** `POST /api/v1/auth/register` (`{"email", "password"}`, 8 to
  128 characters) creates the account and mails a verification link. Its
  token goes to `POST /api/v1/auth/email/verify`; a signed-in user asks for
  a new link with `POST /api/v1/auth/email/verification/resend`. With
  `MAIL_TRANSPORT=log` (the default) the mail is logged, and
  `RUST_LOG=debug` shows its body.
- **Sign-in.** `POST /api/v1/auth/login` answers a session token. Five
  wrong passwords in a row lock an account for 15 minutes, and an
  unverified email is a `403` while `REQUIRE_VERIFIED_EMAIL=true`.
- **Passwords.** `POST /api/v1/auth/password/forgot` (`{"email"}`, always
  `202`) mails a link valid an hour, and `POST /api/v1/auth/password/reset`
  (`{"token", "new_password"}`) sets the new password and lifts any lock. A
  signed-in user changes theirs with `POST /api/v1/auth/password/change`
  (`{"current_password", "new_password"}`) and gets a notice by mail. Both
  sign every older session out. `BREACH_CHECK=true` refuses passwords Have
  I Been Pwned knows.
- **Email change.** `POST /api/v1/auth/email/change`
  (`{"current_password", "new_email"}`, `202`) mails a notice to the old
  address and, when the new one is free, a link to it;
  `POST /api/v1/auth/email/change/confirm` (`{"token"}`) moves the account.
- **Invitations.** An admin's `POST /api/v1/auth/invitations` (`{"email",
  "display_name"?, "role"?}`) creates an account with no password, records
  it in the audit log, and mails a link valid 7 days; the invitee sets a
  password with it through `POST /api/v1/auth/password/reset`.
- **The account.** `GET /api/v1/auth/me` and `GET`/`PATCH
  /api/v1/users/me` are the signed-in user's own.
- **API keys.** `src/features/api_keys/`: `POST /api/v1/api-keys`
  (`{"name", "scope"?, "expires_in_days"?}`) answers a `rok_…` key once
  (only its prefix and SHA-256 are stored); `GET` lists the caller's
  active keys and `DELETE /api/v1/api-keys/{id}` revokes one. A key works
  wherever a session does, as `Authorization: Bearer rok_…` or
  `X-API-Key`, except under `/auth` and `/api-keys` (`403`); a `read`
  key only for `GET`, `HEAD` and `OPTIONS`. At most 25 active keys
  (`409 conflict` past it); `expires_in_days` is 1 to 365.

Every `/auth` route is limited to `AUTH_RATE_LIMIT` requests a minute per
IP (`429` past it). A flow refusing a well-formed request answers `400`
with `invalid_token`, `wrong_password` or `breached_password`; a body of the
wrong shape answers `422`.

## Uploads

`/api/v1/media` holds each signed-in user's files (the hand-written
`media` feature):

- `POST /api/v1/media` takes the raw file as its body (its `Content-Type`
  is ignored), at most `MEDIA_MAX_UPLOAD_BYTES` (10 MiB; `413` past it).
  The type is detected from the bytes and must be one of
  `MEDIA_ALLOWED_TYPES` (JPEG, PNG, WebP, GIF and PDF; else `415`).
  `?filename=` or the `X-Filename` header names the file (255 characters
  at most). It answers `201` with the file's metadata and its `url`,
  `/api/v1/media/{id}/content`.
- `GET /api/v1/media` lists the caller's files, newest first
  (`?page=&per_page=`); `GET` and `DELETE /api/v1/media/{id}` read or
  delete one.
- `GET /api/v1/media/{id}/content` answers the bytes with the detected
  `Content-Type`, `X-Content-Type-Options: nosniff`, and
  `Content-Disposition: attachment` for anything but an image.

Another user's file is a `404`. The upload route has its own `DefaultBodyLimit`,
in place of the 2 MiB `router` sets. The bytes go through the
`Storage` port in `src/shared/storage/` (put, get and delete by key, the
content type kept), over `object_store`, chosen by `STORAGE_URL`:
`file://./storage` (the default, a `storage/` directory where the server
runs), `s3://<bucket>` (the credentials, region and `AWS_ENDPOINT` come
from the `AWS_*` variables) or `memory://`. The tests store in memory.

## The server

`src/app.rs` is the only wiring file. `AppState` (`#[derive(FromRef)]`)
holds the pool, the generated features' `Services`, the JWT keys, the mail
outbox and templates, the breach check and the rate limit; a handler takes
any of them with `State<...>`. `router(state)` merges health, Swagger UI
and every feature's `routes()`, under tower-http's tracing, request ids
(`x-request-id`), CORS (same origin until you allow yours), a 30-second
timeout (`408`) and a 2 MiB body limit (`413`). `serve` runs it with the
mail sender beside it (`tokio::spawn`), both stopped by the same shutdown
signal.

Every other route but health needs `Authorization: Bearer <token>`: an HS256
JWT signed with `JWT_SECRET`, with `sub` (the user's id), `role` (`user`
or `admin`), `tv` (the account's `token_version`) and `exp`.
`src/shared/auth.rs`'s `AuthUser` and `AdminUser` verify it, then check
the account on every request (`queries/session.sql`): a disabled or
deleted account, or a token issued before the account's `token_version`
was bumped, is a `401`, and the role is read from the database, so a role
change applies at once.

- `src/shared/security/` holds argon2id password hashing, single-use
  tokens (only their SHA-256 is stored), the JWT and the breach check.
- `src/shared/mail/` sends mail through an outbox: a flow writes its
  message as an `outbound_mail` row on its own transaction, and the sender
  delivers it (SMTP, or the log), retrying with backoff; after five
  failures a row is `dead`, and loses its bodies and the link in them.
- `src/shared/rate_limit/` is `RateLimit`, a tower `Layer` (`mod.rs`) over
  `governor` (`limiter.rs`, the same in both templates): `AUTH_RATE_LIMIT` requests a minute per client IP, then
  `429 too_many_requests` with `Retry-After`. The IP is the peer, or with
  `TRUST_PROXY=true` the last `X-Forwarded-For` address. Wrap routes in
  `state.rate_limit.clone()`.

## Dependencies

The template's `Cargo.toml` has them. Starting from a package of your own
(`init --template` keeps an existing `Cargo.toml`), add these:

```toml
[dependencies]
argon2 = "0.6"
async-trait = "0.1"
axum = { version = "0.8", features = ["macros"] }
base64 = "0.22"
bytes = "1"
chrono = { version = "0.4", features = ["serde"] }
governor = { version = "0.10", default-features = false, features = ["std"] }
infer = { version = "0.22", default-features = false }
jsonwebtoken = { version = "11", features = ["rust_crypto"] }
lettre = { version = "0.11", default-features = false, features = ["builder", "hostname", "smtp-transport", "tokio1-rustls-tls"] }
object_store = { version = "0.13", features = ["aws"] }
rand = "0.10"
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_path_to_error = "0.1"
sha1 = "0.11"
sha2 = "0.11"
sqlx = { version = "0.9", features = ["runtime-tokio", "postgres", "derive", "uuid", "chrono", "json"] }
thiserror = "2"
tokio = { version = "1", features = ["macros", "net", "rt", "rt-multi-thread", "signal", "sync", "time"] }
tower = { version = "0.5", features = ["util"] }
tower-http = { version = "0.6", features = ["cors", "request-id", "timeout", "trace"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
utoipa = { version = "5", features = ["chrono", "uuid"] }
utoipa-swagger-ui = { version = "9", features = ["axum", "vendored"] }
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
and rate-limit tests need no database. The lints in `Cargo.toml` deny
`unwrap`, `expect`, `panic`, `todo` and leftover debugging.

## What is generated, and what is yours

`AGENTS.md` and `src/features/README.md` explain the layout. In short:

- **Generated** (first line `@generated`; `rok-sqlc g` rewrites them, never
  edit them): `src/db/` (the data layer), `src/shared/{health,pagination,
  response,tenant,validation}.rs`, and every file of `src/features/` but
  `mod.rs` and each feature's `service.rs`.
- **Yours** (first line `Scaffolded once`, or no header): `rok-sqlc.toml`,
  `migrations/`, `queries/`, `src/{lib,main,app,config,error,openapi}.rs`,
  `src/shared/{mod,auth}.rs`, `src/shared/rate_limit/`,
  `src/shared/security/`, `src/shared/mail/`, `src/shared/storage/`,
  `src/features/mod.rs`, the hand-written `src/features/{auth,account,media}/`,
  each feature's `service.rs` (business rules go there), `tests/`, and this README.

`queries/` holds hand-written SQL, one file per feature: `rok-sqlc g` turns
`queries/auth.sql` into `src/db/queries/auth.rs`, one function per query.
Only the feature of that name calls it, whatever tables its queries touch;
`check-arch` reports another feature that does. A file named after no
feature is anyone's: `queries/session.sql` (the check `AuthUser` makes)
and `queries/mail.sql` (the outbox) serve `src/shared/`.

Change the schema with a new migration, adjust `rok-sqlc.toml`, run
`rok-sqlc g`, and the features follow.

## How the schema meets the target

- `role` is a PostgreSQL enum, so the schema `Role` is `user` or `admin`.
- `has_password` is a generated column (`password_hash IS NOT NULL`).
- `password_hash`, `totp_secret`, `avatar_key`, `failed_logins`,
  `locked_until`, `token_version`, `updated_at` and `deleted_at` are hidden
  (`[api.hidden]`): no response carries them.
- The columns the server sets are managed (`[conventions]
  managed_columns`), so no request body takes them; a trigger keeps
  `updated_at` current.
- `deleted_at` is the soft-delete column: `DELETE /users/{id}` marks the row.
- `email` is immutable (`[api.immutable]`): set when an account is created,
  never by `PATCH /users/{id}`, which answers `422` when its body names it.
  An email changes through `POST /api/v1/auth/email/change` and the link
  it mails to the new address.
- `audit_log` is generated as the `audit_log_entry` module
  (`AuditLogEntryResponse`), served at `/audit-log`, read-only.
- `[api.search]` and `[api.sort]` give the users list its `q` and
  `UserSort` (`created_at`, `email`).
- `api_keys` is in the schema for a feature to come, `media` serves the
  hand-written `media` feature, and
  `auth_tokens` (single-use mailed tokens) and `outbound_mail` (the mail
  outbox) for the account flows; none has a generated API (`[api]
  exclude`).
- `token_version` is bumped to sign an account out everywhere: a token
  carrying an older one is rejected.

`tests/contract.rs` lists the differences that remain, each with its
reason: `avatar_url` (uploads, on every `UserResponse`), and the
`?permanent=` and list `sort`/`order` parameters every generated API has.

## Shared with the actix template

The framework-independent files are the same in both templates, byte for
byte: `migrations/`, `queries/`, `src/shared/security/`, `src/shared/mail/`,
`src/shared/storage/`
and each feature's `dto.rs`, `model.rs`, `repo.rs`, `service.rs` and
`service_api.rs`, the hand-written `auth`, `account` and `media` included: they differ only in their handlers and routes. rok-sqlc's repository checks that they stay so.
