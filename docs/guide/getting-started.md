# Getting started

From an empty directory to a running API with Swagger UI. The steps build
the example project, [`docs/examples/api`](../examples/api): two tables,
`projects` and `tasks`, each owned by the caller who created its rows.
Every file this page shows is from that project.

You need:

- rok-sqlc ([Install](../../README.md#install));
- Rust 1.94 or newer, which the generated code needs (SQLx 0.9);
- PostgreSQL;
- a way to run migrations. This page uses SQLx's CLI (`cargo install
  sqlx-cli`); any tool that applies the files in `migrations/` in order
  works.

There are two ways to start. `rok-sqlc init` scaffolds an empty skeleton
that you fill with your own tables, which is what this page does.
`rok-sqlc init --template actix` copies a complete project instead (see
[Starting from the template](#starting-from-the-template)).

## 1. Scaffold the skeleton

```sh
cargo new myapi
cd myapi
rok-sqlc init
```

`init` writes the application skeleton: the wiring, the configuration
reader, the error type, the auth extractors, the OpenAPI document, and the
architecture rules in `AGENTS.md`. Each file is scaffolded: written once,
and yours from then on ([Generated and scaffolded files](layout.md)). It
replaces cargo's hello-world `src/main.rs` and prints what it wrote and
what to do next:

```text
replaced cargo's default src/main.rs
scaffolded AGENTS.md
scaffolded rok-sqlc.toml
scaffolded src/app.rs
scaffolded src/config.rs
scaffolded src/error.rs
scaffolded src/features/mod.rs
scaffolded src/lib.rs
scaffolded src/main.rs
scaffolded src/openapi.rs
scaffolded src/shared/auth.rs
scaffolded src/shared/mod.rs

Next steps:
  1. add the dependencies listed at the end of rok-sqlc.toml to Cargo.toml
  2. add migrations to migrations/ (0001_init.sql, ...)
  3. run `rok-sqlc g` to generate src/db/ and src/features/
  4. set DATABASE_URL and JWT_SECRET, then `cargo run`
```

`init` generates nothing yet, so the project doesn't build until step 3.

## 2. Add the dependencies

The end of `rok-sqlc.toml` lists, as comments, the dependencies the
skeleton needs and the clippy lints that forbid `unwrap`, `expect` and
`panic`. After the first `rok-sqlc g`, the generated `src/db/mod.rs` lists
the ones the generated code needs, with the features your columns call for
(`chrono` for timestamps, for example). The example's `Cargo.toml` is both
lists together, plus `async-trait` for the port its tasks feature declares:

<!-- source: docs/examples/api/Cargo.toml -->
```toml
[dependencies]
# `init`'s hints at the end of `rok-sqlc.toml`, the generated
# `src/db/mod.rs`'s, and async-trait for the port in `tasks/service.rs`.
actix-web = "4"
async-trait = "0.1"
chrono = { version = "0.4", features = ["serde"] }
jsonwebtoken = { version = "11", features = ["rust_crypto"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sqlx = { version = "0.9", features = ["runtime-tokio", "postgres", "derive", "uuid", "chrono"] }
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
utoipa = { version = "5", features = ["chrono", "uuid"] }
utoipa-swagger-ui = { version = "9", features = ["actix-web", "vendored"] }
uuid = { version = "1", features = ["v7", "serde"] }
validator = { version = "0.21", features = ["derive"] }

[lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
```

Set `rust-version = "1.94"` in `[package]` too.

## 3. Write a migration and generate

Each table in `migrations/` becomes part of the data layer (`src/db/`) and,
by default, an API feature (`src/features/<table>/`). Name migration files
so they sort in the order they apply (`0001_projects.sql`, ...): rok-sqlc
reads them in filename order and skips `*.down.sql`.

<!-- source: docs/examples/api/migrations/0001_projects.sql -->
```sql
CREATE TABLE projects (
    id          UUID PRIMARY KEY,
    owner_id    UUID NOT NULL,
    name        VARCHAR(80) NOT NULL,
    open_tasks  INTEGER NOT NULL DEFAULT 0 CHECK (open_tasks >= 0),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE tasks (
    id          UUID PRIMARY KEY,
    owner_id    UUID NOT NULL,
    project_id  UUID NOT NULL REFERENCES projects (id),
    title       VARCHAR(200) NOT NULL,
    done        BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

The example also scopes each table to its owner and names the columns a
list's `q` searches. That is the `[api]` part of its `rok-sqlc.toml`;
[Configuring API features](configuring.md) has every key:

<!-- source: docs/examples/api/rok-sqlc.toml -->
```toml
[api]
output = "src/features"

[api.owner]                  # each caller sees and changes only their own rows
projects = "owner_id"
tasks = "owner_id"

[api.search]                 # the columns the list's `q` matches
projects = ["name"]
tasks = ["title"]
```

Then generate:

```sh
rok-sqlc g
```

It prints one line per file: `generated` for the data layer, the shared
tools and the features, and `scaffolded` for each feature's `service.rs`,
which is yours. Add any dependency the new `src/db/mod.rs` lists that
`Cargo.toml` lacks, and the project builds: `cargo build`.

rok-sqlc reads the SQL files; it never connects to a database to generate.

## 4. Create the database

The application never changes the schema. Migrations are a separate step,
here with SQLx's CLI:

```sh
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/myapi
sqlx database create
sqlx migrate run
```

## 5. Configure and run

The application reads its settings from environment variables, in
`src/config.rs` and nowhere else. Two are required: `DATABASE_URL` and
`JWT_SECRET`, the HS256 key bearer tokens are signed with. The others have
defaults ([Deploying](deploying.md#configuration) lists them).

The application doesn't read a `.env` file. Keep your development settings
in one if you like (SQLx's CLI reads it), and export it into the shell
before `cargo run`:

```sh
# .env
DATABASE_URL=postgres://postgres:postgres@localhost:5432/myapi
JWT_SECRET=change-me
BIND_ADDR=127.0.0.1:8080
```

```sh
set -a; . ./.env; set +a
cargo run
```

The server connects its pool before it listens, so a database it can't
reach stops it ([Troubleshooting](troubleshooting.md#running-the-application)
lists the start-up errors). It listens on `BIND_ADDR` (`0.0.0.0:8080` by
default).

## 6. The first requests

The health checks need no token:

```sh
curl -i http://127.0.0.1:8080/health/ready
```

```http
HTTP/1.1 200 OK
content-type: application/json

{"status":"ready"}
```

Every route of a feature with the default access needs a bearer token, so
a request without one is a `401` in the wire standard's error body:

```http
HTTP/1.1 401 Unauthorized
content-type: application/json

{"error":{"code":"unauthorized","message":"a bearer token is required"}}
```

The scaffolded `src/shared/auth.rs` verifies tokens; it doesn't issue
them. A token is an HS256 JWT signed with `JWT_SECRET`, whose claims are the
caller's id (`sub`, a UUID), their role (`role`, `"user"` or `"admin"`) and
its expiry (`exp`, in seconds since the Unix epoch). Until your application
has a login endpoint, make one for development, for example with OpenSSL:

```sh
b64() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }
header=$(printf '{"alg":"HS256","typ":"JWT"}' | b64)
claims=$(printf '{"sub":"%s","role":"user","exp":%s}' \
  0190a4b2-4c1e-7d3a-9f00-000000000001 $(( $(date +%s) + 3600 )) | b64)
signature=$(printf '%s.%s' "$header" "$claims" \
  | openssl dgst -sha256 -hmac "$JWT_SECRET" -binary | b64)
TOKEN="$header.$claims.$signature"
```

With it, create a project:

```sh
curl -i http://127.0.0.1:8080/api/v1/projects \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"name":"Launch"}'
```

```http
HTTP/1.1 201 Created
content-type: application/json
location: /api/v1/projects/01a1538f-54cf-729e-b214-0e47a960825b

{"id":"01a1538f-54cf-729e-b214-0e47a960825b","owner_id":"0190a4b2-4c1e-7d3a-9f00-000000000001","name":"Launch","open_tasks":0,"created_at":"2026-10-19T09:47:44.718664Z"}
```

The server made the id (`primary_key_ids = "uuid_v7"`, which `init` sets)
and set the owner to the caller. `GET /api/v1/projects` now answers one
page holding it, `{"items": [...], "page": 1, "per_page": 20, "total": 1}`.
[The wire standard](wire-standard.md) describes every answer.

## 7. Swagger UI

Open `http://127.0.0.1:8080/docs/` in a browser. Swagger UI shows every
route with its parameters, bodies and answers, from the document served at
`/api-docs/openapi.json`. Paste the token into **Authorize** to try the
routes that need one. [The OpenAPI document](openapi.md) describes what is
in the document and how to add your own routes to it.

## Starting from the template

```sh
rok-sqlc init --template actix myapi
cd myapi
rok-sqlc g
```

`--template actix` copies [`templates/actix-api/`](../../templates/actix-api)
into the directory instead of the bare skeleton: its `Cargo.toml` and
pinned `Cargo.lock`, its configuration, its migration, the scaffolded
files, its tests and its README. rok-sqlc's generated files are not
copied; `rok-sqlc g` generates them, as the printed next steps say.

Each file is written only where none exists, so the directory can be a new
one or an existing cargo project (after `cargo new myapi`, say). In an
existing project your `Cargo.toml` stays, cargo's hello-world
`src/main.rs` is replaced, and the template's Rust files are rewritten to
name your library crate instead of the template's `actix_api` (`use
myapi::{app, config};`). Add the template's dependencies and lints to your
`Cargo.toml` then, as the first printed step says; the template's
[README](../../templates/actix-api/README.md) lists them and describes the
project.

The template is not the `projects` and `tasks` example of steps 3 to 7: it
serves users and an audit log, and every route but health needs an admin.
From `rok-sqlc g` on:

1. Create the database and apply the template's migration:

   ```sh
   export DATABASE_URL=postgres://postgres:postgres@localhost:5432/myapi
   sqlx database create
   sqlx migrate run
   ```

2. Configure and run. `.env.example` lists every setting:

   ```sh
   cp .env.example .env      # then set DATABASE_URL and JWT_SECRET in it
   set -a; . ./.env; set +a
   cargo run
   ```

3. Make a token as in [step 6](#6-the-first-requests), with
   `"role":"admin"` in its claims. No route creates users yet (sign-up
   comes with a later release), so the list starts empty:

   ```sh
   curl http://127.0.0.1:8080/api/v1/users -H "Authorization: Bearer $TOKEN"
   ```

   ```json
   {"items":[],"page":1,"per_page":20,"total":0}
   ```

   Add one in SQL, then find and change it through the API:

   ```sh
   psql "$DATABASE_URL" -c "INSERT INTO users (id, email, display_name, role)
     VALUES ('0190a4b2-4c1e-7d3a-9f00-000000000001', 'ada@example.com', 'Ada', 'admin')"
   curl "http://127.0.0.1:8080/api/v1/users?q=ada" -H "Authorization: Bearer $TOKEN"
   curl -X PATCH http://127.0.0.1:8080/api/v1/users/0190a4b2-4c1e-7d3a-9f00-000000000001 \
     -H "Authorization: Bearer $TOKEN" \
     -H 'Content-Type: application/json' \
     -d '{"display_name":"Ada L."}'
   ```

   A token whose role is `user` gets `403` with `this needs the admin
   role`. `GET /api/v1/audit-log` answers the audit log the same way.

4. Swagger UI is at `http://127.0.0.1:8080/docs/`, as in
   [step 7](#7-swagger-ui).

`cargo test` runs the template's contract test, which checks the served
OpenAPI document against api-starter-axum's (`tests/spec/openapi.json`) and
needs no database, and its API tests, which run when `DATABASE_URL` is set.

## Next

- [Configuring API features](configuring.md): access, owners, hidden
  columns, search, sort and the other `[api]` keys.
- [Architecture guidelines](architecture.md): where each piece of code
  goes, and the rules `rok-sqlc check-arch` enforces.
- [Customizing the generated API](customizing.md): business rules, ports
  and hand-written features.
- [Testing an API](testing.md).
