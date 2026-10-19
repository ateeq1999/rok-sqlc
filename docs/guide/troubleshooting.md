# Troubleshooting

Each error below is quoted as rok-sqlc or the generated application prints
it, with its cause and its fix. Table, column and file names are examples:
yours will differ. Every error exits with code 1.

## Configuration errors

`rok-sqlc g` checks `rok-sqlc.toml` before it writes anything.

### An unknown key or value

```text
Error: invalid config file rok-sqlc.toml

Caused by:
    TOML parse error at line 4, column 1
      |
    4 | prefixx = "/v2"
      | ^^^^^^^
    unknown field `prefixx`, expected one of `output`, `db_path`, `prefix`, `search_endpoint`, `openapi`, `exclude`, `skip`, `routes`, `access`, `owner`, `hidden`, `immutable`, `search`, `sort`, `rules`
```

A misspelled key. Unknown keys are errors, so a typo can't silently do
nothing; the message lists the valid ones. The same goes for values:

```text
    unknown variant `admins`, expected one of `public`, `user`, `admin`
```

```text
    unknown variant `delete`, expected one of `list`, `get_one`, `create`, `update`, `remove`, `search`, `restore`, `lookups`, `relationships`
```

The first is an `[api.access]` value, the second an `[api.skip]` name
(`remove` is the delete endpoint).

### `api.output` without its derives

```text
Error: `api.output` needs `derive = ["serde", "validate"]` and `actix_web` in `extractors`, since API features take validated bodies through the actix-web extractor
```

Add both to `rok-sqlc.toml`, as `init` writes them:

```toml
derive = ["serde", "validate"]
extractors = ["actix_web"]

[api]
output = "src/features"
```

### An entry naming no table or column

```text
Error: `api.owner.posts` in the configuration names no generated table of the schema
```

```text
Error: `api.owner.articles` in the configuration names `writer_id`, which is not a column of `articles`
```

```text
Error: `api.exclude` in the configuration names `posts`, which is no generated table of the schema
```

The table or column doesn't exist, or no longer does after a migration
dropped or renamed it. Fix the entry. A table key may be the SQL name, the
schema-qualified name or the `[modules]` name; a column is its SQL name.

### Owner columns

```text
Error: `api.owner.articles` in the configuration names `title`, which must be `UUID NOT NULL` to be an owner column
```

An owner column holds the caller's id, a UUID, on every row. Use a
`UUID NOT NULL` column, or change the column in a migration.

```text
Error: `api.owner.articles` in the configuration names `author_id`, but `articles` is public; an owned table needs a caller, so set `api.access.articles` to `user` or `admin`
```

A public route has no caller to own its rows. Drop the owner, or the
`public` access.

### Hidden, search and sort columns

```text
Error: `api.hidden.users` in the configuration names `id`, a key column of `users`, which can't be hidden
```

Routes and `Location` use the key, so it must be in the response.

```text
Error: `api.search.users` in the configuration names `created_at`, which is not a text column, so `q` can't match it
```

`q` matches text columns (`TEXT`, `VARCHAR`, ...). Filter other columns
with `POST /search` (`[api] search_endpoint = true`).

```text
Error: `api.search.users` in the configuration names `password_hash`, a hidden column of `users`, which `q` can't match
```

A hidden column can't be searched or sorted, or its values would leak
through the order or the matches. Remove it from `[api.search]` or
`[api.sort]`.

```text
Error: `api.sort.users` in the configuration names `id`, which can't be sorted: sort columns are numbers, dates, times or text
```

A UUID, a boolean or JSON can't be an `[api.sort]` key. Name another
column.

```text
Error: `pings` has no column its list can sort by; name one in `api.sort`, or skip `list` in `api.skip`
```

A list needs an order to page through, and this table has no key and no
column that sorts. Name a sort column, or skip the list.

### Immutable columns

```text
Error: `api.immutable.users` in the configuration names `id`, a key column of `users`, which no update changes anyway
```

The same goes for the owner and the tenant column. Drop the entry: the
update body never takes those.

```text
Error: `api.immutable.users` in the configuration names `updated_at`, a managed column of `users`, which generated code never writes
```

A managed or generated column is in no request body at all, so it has
nothing to be immutable from. Drop the entry.

### String rules

```text
Error: `api.rules.users.email` in the configuration has `emial`, which is no string rule; the rules are `email`, `url`, `uuid`, `ulid`, `ipAddress`, `mobile`, `alpha`, `alphaNumeric`, `ascii`, `regex`, `in`, `notIn`, `startsWith`, `endsWith`, `minLength`, `maxLength`, `fixedLength`, `hexCode`, `iban`, `jwt`, `postalCode`, `creditCard` and `coordinates`, and the transforms `trim`, `toLowerCase`, `toUpperCase` and `normalizeEmail`
```

A misspelled rule. The names are VineJS's, in its casing.

```text
Error: `api.rules.users.email` in the configuration has `minLength(x)`, but `minLength` takes a number of characters, such as `minLength(3)`
```

```text
Error: `api.rules.users.phone` in the configuration has `mobile(xx-XX)`, but `mobile` takes locales, such as `mobile(ar-SA)`, or none for any, and `strictMode`; the locales are `ar-AE`, `ar-BH`, `ar-EG`, `ar-JO`, `ar-KW`, `ar-OM`, `ar-QA`, `ar-SA`, `de-DE`, `en-CA`, `en-GB`, `en-IN`, `en-US`, `es-ES`, `fr-FR` and `tr-TR`
```

The arguments don't fit the rule. The message says what it takes; for
`mobile`, `postalCode` and `creditCard` it lists the values rok-sqlc
carries.

```text
Error: `api.rules.users.email` in the configuration has `regex(^a$)`, but `regex` takes its pattern in an inline table: `{ regex = "^[a-z0-9-]+$" }`
```

```text
Error: `api.rules.users.email` in the configuration has the pattern `^[a-z`, which is no regular expression: regex parse error:
    ^[a-z
     ^
error: unclosed character class
```

A pattern goes in an inline table, so its commas and parentheses stay
its own, and must parse in the `regex` crate's syntax (no look-around or
backreferences).

```text
Error: invalid config file rok-sqlc.toml

Caused by:
    TOML parse error at line 39, column 10
       |
    39 | email = [{ pattern = "x" }]
       |          ^^^^^^^^^^^^^^^^^
    `pattern`: an inline rule takes one key: `regex`, `in`, `notIn`, `startsWith` or `endsWith`
```

An inline table names the rule it carries the argument of.

```text
Error: `api.rules.users.email` in the configuration has `trim` twice
```

Drop the repeat.

```text
Error: `api.rules.users.password` in the configuration has `sameAs`, which rok-sqlc leaves out: cross-field rules belong in hand-written request types
```

```text
Error: `api.rules.users.website` in the configuration has `activeUrl`, which rok-sqlc leaves out: it looks up DNS records while validating
```

`confirmed`, `sameAs` and `notSameAs` compare two fields, which a
generated body doesn't do; `activeUrl` would make a request wait on DNS.
Write a request type of your own for them (see
[customizing](customizing.md)).

```text
Error: `api.rules.users` in the configuration names `age`, which is not a text column, so string rules can't check it
```

```text
Error: `api.rules.users` in the configuration names `slug`, a generated column, which generated code never writes
```

String rules check text columns that a body holds. A number has the
schema's `range` rule instead (a `CHECK`), and a generated or managed
column is in no body.

```text
Error: `api.rules.users` in the configuration names `nickname`, which is not a column of `users`
```

```text
Error: `api.rules.posts` in the configuration names no generated table of the schema
```

The table or column doesn't exist (any more). Fix the entry.

### Route bases

```text
Error: `api.routes.audit_log` in the configuration is `audit-log`, but a route base must start with `/`
```

```text
Error: `api.routes.audit_log` in the configuration is `/users`, the same route base as the default route base of `users`; route bases must be unique
```

Two features would answer the same URLs. Give one a different base, or
exclude one of the tables.

## Generation

### No schema

```text
error: no schema given: pass --schema <PATH>, or set `schema` in the --json payload or rok-sqlc.toml
```

rok-sqlc found no `rok-sqlc.toml` in the directory or its parents, or the
file sets no `schema`. Run it from the project, or pass `-c`.

```text
Error: schema directory `migrations` contains no `*.sql` migrations
```

A fresh `init` project: write a migration first.

### A file that isn't rok-sqlc's

```text
error: refused to write src/features/users/routes.rs (file exists and was not generated by rok-sqlc; pass --force to overwrite)
Error: nothing was written; 1 file(s) refused (use --force to overwrite)
```

A generated file's path holds a file without the `// @generated by
rok-sqlc` header: your own file, or a generated one whose header was
removed. Nothing was written. Move your file elsewhere (a generated
feature takes no extra files: see [Customizing](customizing.md)), or pass
`--force` to replace it. `--force` never replaces a scaffolded file.

### A query that doesn't fit the schema

```text
error: queries.sql:2: query `Bad`: column `nope` does not exist in `articles`
Error: 1 unsupported query in `queries.sql`; pass --lenient to skip them
```

Fix the query. `--lenient` generates the rest and skips it, with a
warning.

### Drift in CI

```text
Error: generated code is out of date: 9 file(s) drifted; run `rok-sqlc generate` to update them
```

`rok-sqlc g --check` found committed generated code that doesn't match the
schema and configuration (the lines above it list the files). Run
`rok-sqlc g` and commit the result. If you didn't change the schema, check
that CI runs the same rok-sqlc version as you, and that nobody edited a
generated file. [After a schema change](after-a-schema-change.md#in-ci---check)
explains the report.

### The OpenAPI document turned off

```text
warning: `[api] openapi = false`, but src/openapi.rs uses `GeneratedApiDoc`, which is no longer generated: delete it, `pub mod openapi;` in `lib.rs` and the OpenAPI lines of `app.rs` (see "Turning the document off" in docs/guide/openapi.md)
```

Generation succeeded, but the project won't build until you remove the
document's wiring from the files `init` scaffolded:
[Turning the document off](openapi.md#turning-the-document-off).

### An unknown template

```text
error: invalid value 'nope' for '--template <NAME>'
  [possible values: actix]
```

`rok-sqlc init --template` takes `actix`, the only template so far.

## Building

**``file not found for module `db` `` (or `generated`) right after
`init`.** `init` scaffolds the skeleton only; the modules it declares are
generated. Write a migration and run `rok-sqlc g`.

**Unresolved crates (`utoipa`, `chrono`, ...) after `rok-sqlc g`.** A
column type or the document needs a dependency `Cargo.toml` lacks. The
generated `src/db/mod.rs` lists every dependency with its features at the
top; add the missing ones.

**An override no longer matches the trait after a schema change.** See
[After a schema change](after-a-schema-change.md#what-breaks-and-how-to-fix-it).

**`unwrap_used`, `expect_used` or `panic` denied in tests.** The lints
`init` suggests apply to every target. Return a `Result` from the test
and use `?` ([Testing](testing.md)).

## `check-arch`

`rok-sqlc check-arch` prints one line per violation,
`path:line: rule: message`, then a count:

```text
Error: check-arch found 11 violation(s)
```

The rules are explained in [Architecture](architecture.md#4-the-six-rules).
Each violation, with its usual fix:

```text
src/features/users/service.rs:33: rule-1: `actix_web::HttpResponse`: only routes.rs, handlers.rs, app.rs, main.rs, error.rs, features/generated.rs, shared/ and db/ may use actix_web
```

HTTP stays at the edge. A service returns rows and `AppError`s; the
handler builds the response. Move the code into the handler (in a
hand-written feature) or return an error variant.

```text
src/features/users/service.rs:34: rule-2: `sqlx::query`: only repo.rs, model.rs, app.rs, error.rs, features/generated.rs, shared/ and db/ may use sqlx (service.rs and service_api.rs: PgPool and PgConnection only)
```

A service writes no SQL. Put the query in a query file, so the data layer
generates a function for it, and call that.

```text
src/features/users/service.rs:35: rule-4: `crate::features::articles::ArticlesService::new`: feature `users` refers to another feature; declare a port in its service.rs instead
```

Declare a port trait in this feature's `service.rs` and implement it in
`app.rs` ([Customizing](customizing.md#writing-a-port)). To read the other
table, call `crate::db::<table>::read` instead, which is allowed.

```text
src/features/users/service.rs:36: cross-feature-write: `crate::db::articles::write::delete`: feature `users` writes another table; declare a port in its service.rs and implement it in app.rs
```

Writes to another feature's table go through that feature's service, so
its rules apply. Use a port, calling the other service's `_in` method on
your connection.

```text
src/shared/auth.rs:109: rule-5: `crate::features::users::UsersService::new`: shared/ refers to the features
```

`shared/` holds tools with no business meaning. Move the code into a
feature, or pass what the tool needs in as a parameter.

```text
src/features/users/service.rs:38: rule-6: `unwrap()` outside tests: return an error instead
```

Return a `Result<_, AppError>` and use `?`, or `ok_or_else` with an
`AppError` variant. `expect()`, `panic!`, `todo!`, `println!` and `dbg!`
are reported the same way; log with `tracing` instead of printing.

```text
src/features/users/extra.rs:1: layout: feature `users` holds a file it may not: a feature has only mod.rs, routes.rs, handlers.rs, dto.rs, model.rs, repo.rs, service_api.rs, service.rs
```

```text
src/jobs:1: layout: unknown top-level directory `jobs`: src/ holds only bin, db, features, shared
```

```text
src/utils:1: layout: no module may be named `utils`: put the code in shared/ or in a feature
```

Move the code into one of the allowed files: business code into the
feature's `service.rs`, a tool into `shared/`, a worker into `src/bin/`.
While you move a project over, `-l`/`--lenient` prints layout violations
as warnings and fails only on the rules.

Two errors stop `check-arch` before it checks anything:

```text
Error: no rok-sqlc.toml found in the current directory or its parents
```

```text
Error: rok-sqlc.toml sets no `api.output`: check-arch checks an API project
```

Run it from the project, or pass the config file with `-c`.

## Running the application

The binary stops before it listens when its settings are wrong, with one
line on stderr and exit code 1:

```text
error: environment variable `JWT_SECRET` is not set
```

```text
error: environment variable `DB_MAX_CONNECTIONS` is `lots`, which is not a whole number
```

Set the variable ([Deploying](deploying.md#configuration)). The
application doesn't read `.env`; export it into the environment.

```text
error: pool timed out while waiting for an open connection
```

The pool couldn't connect within `DB_ACQUIRE_TIMEOUT_SECS`: the database
is down, or `DATABASE_URL` names the wrong host or port. Check the URL with
`psql "$DATABASE_URL"`.

**Every request is `401` with `the token is invalid or expired`.** The
token was signed with another secret than the server's `JWT_SECRET`, isn't
HS256, has expired (`exp`), or its claims don't match (`sub` must be a
UUID, `role` `user` or `admin`).

**A `500` with `something went wrong`.** The cause is in the server's log
([Deploying](deploying.md#logs)), as an `ERROR` event. A common one in
tests: building the app without the JWT keys or the pool as app data;
build it with `app::configure`, as `main.rs` does. On a tenant-scoped
table, it means no middleware put the tenant in the request's extensions
([Configuring](configuring.md#conventions-that-shape-the-api)).

**`/docs` answers `303`.** Swagger UI lives at `/docs/`, with the slash;
`/docs` redirects there.
