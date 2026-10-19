# Deploying

A generated API is one stateless binary configured by environment
variables. This page covers its configuration, the database pool, health
checks, migrations, logs, and the checks to run in CI. The samples come
from [`docs/examples/api`](../examples/api), whose `config.rs` and
`app.rs` are the ones `rok-sqlc init` scaffolds.

## Configuration

`src/config.rs` is the only file that reads the environment. It reads
every setting once, at start, into one `Config`:

<!-- source: docs/examples/api/src/config.rs -->
```rust
//! | Variable | Default | Setting |
//! |---|---|---|
//! | `DATABASE_URL` | required | the PostgreSQL URL |
//! | `BIND_ADDR` | `0.0.0.0:8080` | where the server listens |
//! | `JWT_SECRET` | required | the HS256 key tokens are signed with |
//! | `DB_MAX_CONNECTIONS` | 20 | pool size |
//! | `DB_MIN_CONNECTIONS` | 5 | idle connections kept open |
//! | `DB_ACQUIRE_TIMEOUT_SECS` | 3 | wait for a connection |
//! | `DB_IDLE_TIMEOUT_SECS` | 600 | close an idle connection after |
//! | `DB_MAX_LIFETIME_SECS` | 1800 | replace a connection after |
```

A missing required variable, or a number that doesn't parse, stops the
binary before it listens, naming the variable:

```text
Error: Custom { kind: Other, error: Missing("JWT_SECRET") }
```

`Config` has no `Debug`, so the secret can't reach a log. It reads no
`.env` file: your platform (a container's environment, a secret store, a
systemd unit) sets the variables. A setting of your own (a mail server, a
bucket) is a field and a line in `Config::from_env`, in the same file.

## The pool

`DB_*` fill the generated `PoolSettings`, and `app.rs` opens the pool with
them before the server listens. It connects at once, so a database it
can't reach stops the binary instead of failing the first request:

<!-- source: docs/examples/api/src/db/pool.rs -->
```rust
impl Default for PoolSettings {
    /// At most 20 connections, 5 kept open, 3 seconds to get one, closed
    /// after 10 idle minutes, replaced after 30.
    fn default() -> Self {
        PoolSettings {
            max_connections: 20,
            min_connections: 5,
            acquire_timeout: Duration::from_secs(3),
            idle_timeout: Duration::from_secs(600),
            max_lifetime: Duration::from_secs(1800),
        }
    }
}
```

Size the pool for the database, not the instance: every copy of the API
opens up to `DB_MAX_CONNECTIONS`, and together they must stay under the
server's `max_connections`, with room for migrations and admin sessions.
`PoolSettings::for_hardware(cpu_cores, spindles)` gives PostgreSQL's rule
of thumb, `cpu_cores * 2 + spindles`, if you'd rather size it in code. A
request that waits longer than `DB_ACQUIRE_TIMEOUT_SECS` for a connection
fails with `500 internal_error`.

## Health checks

| Route | Answers | Use it for |
|---|---|---|
| `GET /health/live` | `200 {"status":"ok"}` while the process answers | the liveness probe: restart the instance when it fails |
| `GET /health/ready` | `200 {"status":"ready"}` when the database answers `SELECT 1`; `503` with code `service_unavailable` when it doesn't | the readiness probe: send traffic only while it passes |

Keep the database out of the liveness probe. A database outage makes every
instance unready, which stops traffic; restarting them all would not help
and would add a reconnect storm. Both routes sit outside the API prefix and
need no token, so a load balancer can call them.

## Migrations

The application never changes the schema. Run the migrations as a
separate step before the new version starts, with the tool you develop
with (`sqlx migrate run`, for example), from a release job or an init
container:

```sh
DATABASE_URL=postgres://... sqlx migrate run
```

While a deploy rolls out, the old and the new version run side by side
against the new schema, so keep each migration backward compatible: add a
column as nullable or with a default, and drop or rename one only in a
later release, once no running version reads it. Never edit a migration
that has been applied: write a new one. [After a schema
change](after-a-schema-change.md) covers the code side.

## Logs

The scaffolded code logs with [`tracing`](https://docs.rs/tracing): the
pool's settings at start, the bind address, every `500`'s cause, and a
failed readiness check. `main.rs` installs no subscriber, so until you add
one those events go nowhere. Add `tracing-subscriber` and initialize it at
the top of `main` (JSON output, for example, for a log collector), before
`Config::from_env`.

## Checks in CI

Run these on every change, in order of speed:

```sh
cargo fmt --check
rok-sqlc g --check
rok-sqlc check-arch
cargo clippy --all-targets -- -D warnings
cargo test
```

- `rok-sqlc g --check` regenerates in memory and fails, listing the files,
  when the committed generated code doesn't match the migrations, the
  query files and `rok-sqlc.toml`: someone changed the schema without
  running `rok-sqlc g`, or edited a generated file. It never checks the
  contents of a scaffolded file, which are yours.
- `rok-sqlc check-arch` fails on any break of the architecture's rules
  ([Architecture](architecture.md#4-the-six-rules)), printing
  `path:line: rule: message`.
- `cargo test` needs a PostgreSQL with `DATABASE_URL` set
  ([Testing](testing.md#in-ci)).

Install rok-sqlc in CI from a release binary or with `cargo install
rok-sqlc --locked`, pinned to the version the project generates with, so
`--check` compares against the same generator.

## Building the binary

The application is a library with a thin `main.rs`, so the release build
is `cargo build --release` and the binary is the package's name. It serves
the OpenAPI document and Swagger UI from the binary itself (the `vendored`
feature of `utoipa-swagger-ui`), so the build needs no network for them and
the image needs no extra files.
