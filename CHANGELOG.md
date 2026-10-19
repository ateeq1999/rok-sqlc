# Changelog

All notable changes to rok-sqlc are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Changes to
the *generated* API count as breaking (see ADR 0012).

## [Unreleased]

### Breaking changes to the generated API

- `extractors`: `db/http/` answers a rejected body with the wire standard
  of ADR 0016, `{"error": {"code", "message", "details"?}}`, on both axum
  and actix-web. The `success` field is gone and the codes are snake_case:
  `400` `bad_request` (was `MALFORMED_BODY`), `413` `payload_too_large`
  (was `PAYLOAD_TOO_LARGE`) and `422` `validation_failed` (was
  `VALIDATION_FAILED`). In `db::http`, `ErrorBody` loses `success`,
  `ErrorPayload` serializes `code` before `message`, the constant
  `MALFORMED_BODY` is now `BAD_REQUEST`, and `ErrorBody::new(code, message)`
  builds a body for any other error answer.

### Added

- `[api.immutable]`: columns set on create and never changed by the
  generated update, by table, in the config file and the JSON payload
  (`ApiConfig::immutable`). They stay in `CreateXDto` and the response,
  leave `UpdateXDto` and its OpenAPI schema, and a `PATCH` naming one
  answers `422 validation_failed` with the detail code `immutable`. The
  data layer's `UpdateX` keeps them. A key, owner, tenant or managed
  column fails the run. The actix template makes `users.email` immutable,
  so its contract no longer lists the update body's `email`.
- The Release workflow publishes the crate to crates.io once every binary
  builds, using the `CARGO_REGISTRY_TOKEN` repository secret. A version
  crates.io already has is skipped, so running a tag again doesn't fail.
- The `[api]` configuration for generated API features (ADR 0016):
  `output` (`-a/--api-output`), `db_path`, `prefix`, `search_endpoint`,
  `exclude`, and the table-keyed `[api.skip]`, `[api.routes]`,
  `[api.access]`, `[api.owner]`, `[api.hidden]`, `[api.search]` and
  `[api.sort]`, in the config file and the JSON payload. The library takes
  it as `Config::api` (`ApiConfig`, `ApiAccess`, `ApiEndpoint`), checked
  against the schema: an owner on a `public` table, or a search or sort
  column that is hidden, fails the run.
- With `api.output`, a run generates the infrastructure of ADR 0016:
  `db/pool.rs` (`PoolSettings`, `init_pool`, `check_health`) and, in
  `shared/` beside the features directory, `response.rs`
  (`PaginatedResponse` and the error body), `pagination.rs` (`ListQuery`,
  `Pagination`, `SortOrder`), `validation.rs`, `tenant.rs` (`Tenant<T>`) and
  `health.rs` (`/health/live`, `/health/ready`). Paths into the data
  layer follow `api.db_path`.
- API features (ADR 0016): with `api.output`, every table but join tables
  and `api.exclude`d ones gets `api.output/<module>/` with `mod.rs`,
  `routes.rs`, `handlers.rs`, `dto.rs` (`CreateXDto`, `UpdateXDto`,
  `XResponse`, `ListXsQuery`), `model.rs`, `repo.rs`, `service_api.rs` (the
  `XsServiceApi` trait with a default per endpoint, each write split into
  `<write>_in` on the caller's connection and `<write>` in a transaction)
  and a scaffolded `service.rs`; plus `generated.rs` (`register_services`,
  `configure_all`) and `README.md`. Endpoints: `GET /` (`page`, `per_page`,
  `q`, `sort`, `order`), `POST /` (`201` with `Location`), and `GET`,
  `PATCH` and `DELETE /{key}`; `[api.skip]` drops any. The data layer
  gains `<table>/listing.rs` (`XSort`, `list_matching`, `count_matching`)
  and `search::push_contains_any`; `shared/` gains `response::path_segment`
  and `validation::{path_config, query_config}`. Generated files under
  `api.output` and `shared/` are removed once stale; scaffolded ones never
  are. The scaffolded service has `XsService::new(pool)`, which
  `register_services` calls.
- Access, owners and hidden columns in API features: handlers take
  `AuthUser` (`[api.access]` `user`, the default; `401` without a valid
  token) or `AdminUser` (`admin`; `403` for a caller who isn't an admin),
  or no caller (`public`). On an owned table (`[api.owner]`) the service
  methods take the `AuthUser`, every read is scoped to the caller's id,
  `update_in` and `remove_in` lock the caller's row before writing (another
  caller's row is a `404`), and `create` sets the owner, which neither body
  holds (`CreateXDto::into_new(owner)`). `[api.hidden]` columns stay in
  both bodies but never appear in `XResponse`. `listing.rs` gains
  `get_owned` and `lock_owned`.
- API features serve the rest of ADR 0016's routes, each skippable in
  `[api.skip]`: `GET /by-<column>/{value}` per single-column unique key that
  isn't hidden; `POST /search` (`{filter, sort, order, page, per_page}`)
  with `api.search_endpoint`, whose filter is the data layer's less hidden
  columns (a `422` naming the field) and relationship filters; relationship
  routes to tables with a feature (`GET /{key}?include=<name>` for a to-one,
  `GET /{key}/<name>` for a to-many, answering a bare JSON array, and for
  a many-to-many `PUT /{key}/<name>` sync and `PUT`/`DELETE
  /{key}/<name>/{other}`), answering
  through the related feature's response under its access and owner; and
  on soft-delete tables `POST /{key}/restore` and `DELETE
  /{key}?permanent=true`. Versioned tables answer `ETag` and need `If-Match`
  on `PATCH` (`428` without, `412` when stale); tenant-scoped tables take
  `Tenant<T>`. `shared/` gains `response::etag` and
  `validation::{IfMatch, IdList}`; `generated.rs` re-exports the responses
  relationship routes use, and `listing.rs` gains the search reads,
  `owned_by`/`owned`/`retain_owned`, and the join tables' writes.
- `rok-sqlc init [DIR]` scaffolds the actix-web application skeleton of
  ADR 0016 (`rok-sqlc.toml`, `migrations/`, `src/{lib,main,app,config,error}.rs`,
  `src/shared/{mod,auth}.rs`, `src/features/mod.rs` and `AGENTS.md`), each
  file only when absent. It generates nothing. The application is a library
  crate (`lib.rs` declares the modules `pub`, `main.rs` starts it), so a
  fresh project passes `cargo clippy --all-targets -- -D warnings`.
- `rok-sqlc init` after `cargo new` replaces cargo's untouched default
  `src/main.rs` (hello world) with the scaffolded one and says so; any other
  `src/main.rs` is kept.
- `rok-sqlc check-arch` (`a`) checks a project against ADR 0016's
  architecture, reading `api.output` and `api.db_path` from
  `rok-sqlc.toml` (`-c/--config`): the feature layout (eight files, no
  `utils`/`helpers`/`common`, no unknown top-level directory under `src/`;
  warnings under `-l/--lenient`) and, parsing the source with `syn`, rules
  1, 2, 4, 5 and 6 and the cross-feature write rule. It prints each
  violation as `path:line: rule: message` and exits 1 if there are any.
  `init`'s `AGENTS.md` states exactly the rules it enforces.
- Scaffolded files: a planned file starting with the scaffold header is
  written only when absent, never rewritten, never reported as changed and
  never stale; `--check` reports a missing one. `Report::scaffolded` lists
  the ones written.
- `primary_key_ids = "cuid2"` (`IdGeneration::Cuid2`, ADR 0016) makes new
  text primary keys with `cuid::cuid2()` (24 characters): the typed id gets
  `generate()`, and `create` fills a key the input leaves out. `UUID` keys
  are left alone. `db/mod.rs` hints `cuid = "2"`.
- The API guide in `docs/guide/`: the architecture guidelines, with the
  rules `check-arch` enforces, and which files are generated, scaffolded or
  yours. The README gains a "Generated APIs" section. Their samples come
  from `docs/examples/api`, a project started with `init`, which the test
  suite regenerates, compiles and runs `check-arch` on.
- The OpenAPI document of generated API features (ADR 0016), with utoipa 5
  and utoipa-swagger-ui 9, on by default and off with `[api] openapi =
  false` (`ApiConfig::openapi`), which leaves output with no utoipa at all.
  Every type a feature exposes derives `ToSchema` (query strings
  `IntoParams`), as do the data layer's typed ids, enums, sort keys, search
  conditions and `ErrorBody`. Every generated handler carries
  `#[utoipa::path]`: its absolute path, a tag per feature (the route base),
  the handler's name as operation id, its params and body,
  `security(("bearer_auth" = []))` when it needs a caller, and exactly the
  statuses it answers, each error with `ErrorBody`. `generated.rs` gains
  `GeneratedApiDoc` and the `BearerAuth` modifier (HTTP bearer, JWT).
  `init` scaffolds `src/openapi.rs` (`ApiDoc` for hand-written paths and
  `document()`, merging the generated document in), and its `app.rs`
  serves `/api-docs/openapi.json` and Swagger UI at `/docs/`. The
  dependency hints add `utoipa` (with the features the columns need) and
  `utoipa-swagger-ui` with `vendored`, so builds need no network. Health's
  handlers are `liveness` and `readiness`, the target document's
  operation ids. The guide gains an OpenAPI page.
- A list takes and documents `q` only when its table has `[api.search]`
  columns: the others' query string is the new shared `PageQuery`, which
  has no `q` (an unknown parameter is ignored, as before). The `bearer_auth`
  scheme has a description, and no OpenAPI description shows rustdoc link
  syntax.
- With `[api] openapi = false`, `rok-sqlc g` warns while the scaffolded
  `src/openapi.rs` still uses `GeneratedApiDoc` (`Report::warnings` in the
  library), and the OpenAPI guide lists the scaffolded lines to remove.
- The OpenAPI document reads like the target: `PaginatedResponse.total` has
  `minimum: 0`, and path parameters are described as "<Singular> id"
  ("User id") for a single key and "<Singular> <column words>" for a
  composite key's columns and a unique lookup's value; a many-to-many
  route's `{other}` is the related row's id.
- `rok-sqlc init --template <NAME>` (`-t`) copies a ready-made project from
  the templates built into the binary (`templates/` in the repository; the
  only one is `actix`), each file only when absent and without its
  generated files, and prints the next steps. An unknown name lists the
  valid ones.
- The `actix` template (`templates/actix-api/`): the `users` and
  `audit-log` part of the axum starter's API, generated from one schema,
  with a contract test that checks the document it serves against the
  starter's (`tests/contract.rs`), API tests against PostgreSQL, and a
  README. CI regenerates it and fails on drift, and runs its lint gate,
  `check-arch` and tests.
- The rest of the API guide: getting started, configuring API features,
  customizing the generated API, the wire standard, testing, deploying,
  after a schema change, and troubleshooting, linked from the README.
  `docs/examples/api` gains a hand-written feature (`stats`, with its own
  OpenAPI path) and service and API tests with `#[sqlx::test]`, which the
  test suite runs when `ROK_SQLC_TEST_DATABASE_URL` is set.

## [0.3.0] - 2026-10-17

Validated bodies and HTTP extractors (ADR 0015). A run without the new
`validate` derive and `extractors` key generates byte-for-byte what 0.2.1
generates.

### Added

- `derive = ["serde", "validate"]` (`-d validate`, `Derive::Validate`) adds
  `validator::Validate` (validator 0.21) to every create input and update
  patch, with rules taken from the schema and English messages naming the
  column (`"name must be at most 80 characters"`):
  - `VARCHAR(n)`, `CHARACTER VARYING(n)`, `CHAR(n)` and `CHARACTER(n)` give
    `length(max = n)`, counted in characters;
  - a single-column `CHECK` comparing `char_length(col)`,
    `character_length(col)` or `length(col)` with an integer gives a
    `length` bound, combined with the declared length (the tighter bounds
    win, and `equal` when they meet);
  - a single-column `CHECK` comparing an integer or float column with a
    literal (`>=`, `>`, `<=`, `<`, either side), `BETWEEN`, or an `AND` of
    these gives `range`. A strict bound on an integer becomes inclusive
    (`> 0` is `min = 1`); on a float it stays strict (`exclusive_min`), and
    one that can't be represented exactly is dropped;
  - on a `NUMERIC` column the range is a `custom` rule calling a generated
    `<field>_in_range` function, with the code `range` and `range`'s
    `min`/`max` params, since validator's `range` can't compare `Decimal`;
  - a typed id over text implements `validator::ValidateLength<u64>`, and a
    typed id over an integer `validator::ValidateRange`, so a column's
    `length` or `range` rule applies through the id and through foreign keys
    using it. A typed id over `NUMERIC` gets no range rule.

  Patch fields are checked only when present. Other `CHECK`s, `NOT NULL`,
  enums and value lists are not rules. `db/mod.rs` names the `validator`
  dependency. `validate` without `serde` fails the run.
- `extractors = ["axum", "actix_web"]` (the JSON payload's `extractors`, the
  repeatable `-e`/`--extractor <axum|actix_web>` flag, and
  `Config::extractors` with the new `#[non_exhaustive]` `Extractor`, part of
  the stable library API) generates `db/http/`, which needs `validate`:
  - `db::http::axum::ValidatedJson<T>` (axum 0.8) and
    `db::http::actix_web::ValidatedJson<T>` (actix-web 4), extractors for
    any `T: DeserializeOwned + Validate`: a generated input or patch, or a
    hand-written request type. Each reads the body through its framework's
    own JSON extractor (`axum::Json<T>`, `web::Json<T>`), so axum's
    `DefaultBodyLimit` and actix-web's `JsonConfig` (limit and
    `Content-Type` predicate) apply; both limits default to 2 MiB;
  - `BodyRejection`, answered in the Rok error envelope
    `{"success": false, "error": {"message", "code", "details"?}}`: `400`
    `MALFORMED_BODY` for a missing or wrong `Content-Type`, bad JSON syntax,
    or a body that can't be read; `413` `PAYLOAD_TOO_LARGE` for a body over
    the framework's size limit, with the message
    `"the request body is over the size limit"` from both; `422`
    `VALIDATION_FAILED` with serde's message and no `details` for JSON that
    doesn't fit the type; `422` `VALIDATION_FAILED` with validator's errors
    as `details` for a broken rule. Each framework's rejection is classified
    in its module, with the message taken from serde's own error (never the
    framework's text), so both answer the same body with the same status,
    code and message;
  - `ErrorBody`, `ErrorPayload`, the three codes,
    `BodyRejection::from_serde`, and `validated(body)`, which checks a
    deserialized body against its rules.

  With actix-web, no `JsonConfig` error handler is needed. `db/mod.rs` names
  the `axum`, `serde_path_to_error` (whose error wraps serde's inside axum's
  rejection), `actix-web` and `serde_json` dependencies. `extractors` without
  `validate` fails the run, and an unknown framework lists the valid ones.
  With `extractors`, `http` is a reserved module name: a table whose module
  would be `http` fails with an error that says so (rename it with
  `[modules]`). Without `extractors`, such a table generates as before.
- The schema model keeps each text column's declared length and the simple
  `CHECK` bounds above, through `CREATE TABLE`, `ADD COLUMN`, `ALTER COLUMN …
  TYPE` (which replaces or clears them), `ADD CONSTRAINT`, and renames.
- The README has a "Validated bodies and HTTP extractors" section, from the
  new example crate `docs/examples/validated-api`.

## [0.2.1] - 2026-10-17

Additive: everything that generated with 0.2.0 generates the same way.

### Added

- `[conventions] primary_key_ids` (`Conventions::primary_key_ids`, the new
  `IdGeneration`): `"uuid_v4"`, `"uuid_v7"`, or `"ulid"` makes new primary
  keys in the generated code for tables keyed by one `UUID` column (or, with
  `"ulid"`, one text column). The typed id gets `generate()`, the create
  input takes the key as `Option<Id>`, and `None` inserts a new id, even over
  a database default. `db/mod.rs` names the `uuid` features or the `ulid`
  crate it needs. Off by default and not in the `rok` preset.
- `MONEY` (`PgMoney`), `INT4RANGE`, `INT8RANGE`, `NUMRANGE`, `DATERANGE`,
  `TSRANGE`, `TSTZRANGE` (`PgRange<T>`), `INET` and `CIDR`
  (`ipnetwork::IpNetwork`), and `MACADDR` (`mac_address::MacAddress`), in
  columns and query-file casts, with equality filters. The dependency hints
  name SQLx's `ipnetwork` and `mac_address` features. `PgMoney` and
  `PgRange` have no serde support, so like `INTERVAL` they fail a run with
  `derive = ["serde"]`.
- Partial unique indexes as `ON CONFLICT` targets in query files, when the
  target repeats the index's condition:
  `ON CONFLICT (email) WHERE deleted_at IS NULL DO …`. The soft-delete
  convention's live-row indexes can now be upsert targets. A target that
  matches only a partial index says which `WHERE` to add.
- `WITH RECURSIVE` in query files: a recursive CTE (`anchor UNION [ALL]
  term`) takes its columns from the anchor, and the recursive term must
  match them.
- `ALTER TABLE` forms that leave the model as it is no longer fail their
  table: `ALTER COLUMN … SET STATISTICS`, `SET STORAGE`, `SET COMPRESSION`,
  `SET`/`RESET (…)`, `SET GENERATED`, `RESTART` and other identity options;
  `RESET (…)`, `CLUSTER ON`, `SET WITHOUT CLUSTER`, `SET TABLESPACE`,
  `SET [UN]LOGGED`, `[NO] FORCE ROW LEVEL SECURITY`, `ALTER CONSTRAINT`,
  `SET (…)`, `REPLICA IDENTITY`, `OWNER TO`, `ENABLE`/`DISABLE
  TRIGGER`/`RULE`, and `VALIDATE CONSTRAINT`.
- `ALTER COLUMN … DROP IDENTITY` and `DROP EXPRESSION` act like `DROP
  DEFAULT`, and `TYPE … COLLATE` like `TYPE`.
- `ALTER TABLE … RENAME CONSTRAINT` renames a primary key or unique
  constraint, and `ALTER INDEX`, `ALTER SEQUENCE`, `ALTER VIEW` and `ALTER
  MATERIALIZED VIEW … RENAME TO` are followed for default constraint names.
  Renaming a key constraint's index renames the constraint.
- A migration with a `BEGIN ATOMIC` function or procedure body, a `DO`
  block, or another `ALTER` of an index, sequence or view that the SQL
  parser can't read is now read past instead of stopping the run.
- Prebuilt binaries. Pushing a `vX.Y.Z` tag runs the Release workflow, which
  builds rok-sqlc for Linux (x86_64 and aarch64, static musl), macOS (x86_64
  and Apple silicon), and Windows (x86_64 and ARM64) and publishes the
  archives, each with a `.sha256` file and a combined `SHA256SUMS`, as a
  GitHub release whose notes are this file's section for the version.
- `install.sh` (Linux, macOS) and `install.ps1` (Windows) install the binary
  for the current platform from a GitHub release and verify its checksum.
- Release builds strip debug symbols, which shrinks the binary.

## [0.2.0] - 2026-10-17

### Breaking changes to the generated API

- On a table with the tenant column, `{Table}Filter` has no field for the
  column (including its `*_is_null` field) and `{Table}SortKey` no variant
  for it. `search` is already scoped to the tenant parameter, so these could
  only repeat the scope or match nothing. Relationship filters are unchanged.
- An enum primary key gets a typed id wrapping the enum
  (`PriorityNoteId(pub types::TicketPriority)`), which is `Copy`, `Eq` and
  `Hash` but not `Ord` or `Display`. The key column, key parameters, the
  `search_after` cursor, and foreign keys into the table take it instead of
  the bare enum.
- With `derive = ["serde"]`, a patch deserialized from JSON tells an absent
  field (leave the column alone) from `null` (set it to `NULL`); before, both
  read as `None`. Serializing a patch skips its `None` fields, so `{}` is an
  empty patch. A nullable input field with a default reads `null` as
  `Some(None)` (insert `NULL`) the same way. Both use the new generated
  `db/nullable.rs`, so a table can no longer be named `nullable`.
- With `derive = ["serde"]`, `{Table}Filter` (including its relationship
  fields), `{Table}SortKey`, and `Direction`, `Sort`, `Ordered`, `Equality`,
  and `Text` in `db::search` derive `Serialize` and `Deserialize`. A filter
  field absent from the JSON adds no condition. The enums among them use
  snake_case variants in JSON (`{"ge": 400}`, `"desc"`, `"price_cents"`).
- A `SERIAL`, `BIGSERIAL`, or `SMALLSERIAL` column off the primary key is
  `NOT NULL`, as in PostgreSQL: its row field is `i32`/`i64`/`i16` instead
  of `Option<…>`, and inserts still leave it to its sequence. After
  `ALTER COLUMN … DROP DEFAULT` it is a required `NOT NULL` input, and a
  managed one has no source of values, which fails its table.

### Added

- The `[modules]` config section (also in the JSON payload, and
  `Config::modules` in the library) renames a table's module, by
  schema-qualified or SQL name: `"audit.users" = "audit_users"`. The row
  struct, typed id, and key struct follow the new name, so same-named tables
  of two schemas both generate, each with its own typed id. An entry naming no
  table, or a name that isn't a snake_case Rust identifier, fails the run.
- Generation fails with a diagnostic when a resource would import two typed
  ids of one name (from tables like `user` and `users`), or an imported typed
  id named like one of the resource's own items, or any file would import
  two other items of one name (the row structs and filters of `category` and
  `categories`, say). Before, the generated code didn't compile.
- `INTERVAL` maps to `sqlx::postgres::types::PgInterval`, `BYTEA` to
  `Vec<u8>` (borrowed as `&[u8]` in key, lookup, and query parameters), and
  `TIME WITH TIME ZONE`/`TIMETZ` to `sqlx::postgres::types::PgTimeTz`, in
  columns, arrays, query-file casts (`$1::interval`), and result columns.
  All three get equality filters: SQLx implements no `PartialOrd` for
  `PgInterval` or `PgTimeTz`.
- With `derive = ["serde"]`, a table or query that would put `PgInterval` or
  `PgTimeTz` (which implement no serde traits) into a serde-deriving item
  fails the run with an error naming the column or parameter.
- In query files, a cast to an enum type infers the generated enum:
  `$1::product_status` and `$1::app.ticket_state` take `types::X`, and
  `$1::product_status[]` takes `&[types::X]`. Such casts were rejected.
- `ALTER TYPE x RENAME TO y` renames the enum: its Rust name follows the new
  name (schema-prefixed only on a clash, as for `CREATE TYPE`), its
  `sqlx(type_name)` becomes `y`, and the columns using it keep working. The
  clause used to fail every table using the type.
- `ALTER TABLE … ALTER COLUMN` is applied instead of failing the table:
  `[SET DATA] TYPE` (with or without `USING`), `SET`/`DROP NOT NULL`,
  `SET`/`DROP DEFAULT`, and `ADD GENERATED … AS IDENTITY`. The column ends
  up in its final shape, and its primary key, unique keys, and typed id
  follow a type change. A change rok-sqlc can't apply (a type with no
  mapping, a serial type, `DROP NOT NULL` on a primary key column, identity
  on a nullable column, a missing column) still fails the table with a
  `path:line` diagnostic. Dropping a managed column's default can now leave
  it without a source of values, which fails its table.

### Changed

- A table that a `--lenient` run skips keeps the resource directory an
  earlier run generated for it, byte for byte, and `db/mod.rs` keeps
  declaring it (and `UpdateOutcome`, when its `update` uses it). Those files
  are no longer stale: a successful run does not remove them, and a lenient
  `--check` reports the table's warning but no drift. A table the schema no
  longer defines (dropped, or never created) is still stale (ADR 0001).
  The rest of the output is generated against the kept table's last good
  shape (its model before the statement that broke it), so the crate still
  compiles: `types.rs` keeps its enums, foreign keys into it keep its typed
  id, and relationships and filters still reach it. The kept directory is
  the table's module, `[modules]` renames included. A table that fails at
  its own `CREATE TABLE`, or whose value lists can't be generated, has no
  such shape, so its earlier files are stale.
- A managed column with no source of values fails its table with a
  `path:line` diagnostic: one that is `NOT NULL`, has no `DEFAULT`, is not
  serial, identity, or `GENERATED`, on a table with no row-level
  `BEFORE INSERT` trigger. Inserts leave managed columns out, so such a
  table could never be inserted into. `CREATE TRIGGER` and `DROP TRIGGER`
  are now read to find those triggers (per table; function bodies are not
  inspected). Nullable managed columns are unaffected. When a later
  migration takes the source away (`ALTER COLUMN … DROP DEFAULT`, or
  `DROP TRIGGER` of the last `BEFORE INSERT` trigger), the diagnostic
  points at that statement.
- `--resource` (and the `resource` key) is described as what it does: it
  generates the named table and regenerates every resource already
  generated in the output directory; tables not generated yet stay
  ungenerated.
- An `ALTER TABLE` the SQL parser can't read (`ALTER COLUMN … DROP
  IDENTITY`, `SET STATISTICS`, `SET STORAGE`, `DROP EXPRESSION`,
  `SET GENERATED`, `TYPE … COLLATE`) fails only the table it alters, at its
  `path:line`, so `--lenient` skips that table and reads on. Any other
  statement the parser can't read still stops the run, now at its
  `path:line`. Before, either stopped the run with a syntax error for the
  whole file.

### Fixed

- A joined read (`get_with_<relationship>`) of a table whose column names
  come within two bytes of PostgreSQL's 63-byte identifier limit works:
  such a column is selected under a shortened alias that names its
  position (`t3_<name>`), where the `t_`/`r_` prefix used to make
  PostgreSQL cut two aliases to the same name. The `is_empty` of a patch and
  the `column` of a sort key are also laid out as rustfmt lays them out for
  such long names.
- `ON CONFLICT ON CONSTRAINT` accepts the names PostgreSQL gives unnamed
  primary key and unique constraints when a default name is already taken
  by another relation in the same schema (`<table>_pkey1`): a table, another
  table's constraint, an index (named, or PostgreSQL's default
  `<table>_<columns>_idx`), a sequence (`CREATE SEQUENCE`, or a serial
  column's `<table>_<column>_seq`), or a view or materialized view, until it
  is dropped. Before, only the table's own constraints counted.
- A table whose search names would collide only through the tenant column
  generates: the tenant column has no `*_is_null` filter field and no sort
  key variant, so it collides with nothing. The check now runs after the
  conventions are applied.

## [0.1.0] - 2026-10-17

First release on crates.io. The entries below include generated-API changes
that break code written against earlier development snapshots.

### Breaking changes to the generated API

- Generated code targets SQLx 0.9 instead of 0.8: the consuming crate needs
  `sqlx = "0.9"` (same feature names) and Rust 1.94 or newer, SQLx 0.9's
  minimum. Search code takes `&mut sqlx::QueryBuilder<sqlx::Postgres>`, which
  has no lifetime parameter in 0.9.
- Struct names are singular: `Product`, `NewProduct`, and `UpdateProduct`
  for a `products` table, instead of names built from the plural table name.
- Generated CRUD functions take a generic `executor: E` (any
  `sqlx::PgExecutor`, such as a pool, a connection, or a transaction) as
  their first argument, instead of a concrete `&PgPool`.
- `get_by_id`, `exists_by_id`, `update`, and `delete` take one parameter per
  primary-key column, so tables with composite keys get the same shape as
  single-key tables.
- `read::search_after` takes a typed `Limit` (clamped to
  `1..=MAX_PAGE_SIZE`, 500) instead of a bare integer. `Page` is built on
  `Limit`, and `Page::next` saturates instead of overflowing.
- `read::search` is generated only when the table has at least one sort key,
  and `filter.rs` only when `search` or `search_after` exists.
- `list` orders rows by primary key.
- Generated SQL keeps schema qualifiers (`app.products`) and lists columns
  explicitly; partial updates and inserts with defaulted columns are built
  with `sqlx::QueryBuilder`.
- `NUMERIC`/`DECIMAL` columns map to `rust_decimal::Decimal`.
- With more than one query input (or a query directory), queries are
  generated as `db/queries/<stem>.rs`, one module per file, instead of a
  single `db/queries.rs`. A lone query file still generates `db/queries.rs`.
- Text columns restricted by a value-list `CHECK` use a generated enum
  (`types::OrderStatus`) instead of `String`.
- Text keys are borrowed: `get_by_id`, `exists_by_id`, `update`, `delete`,
  and unique-key lookups take a text key as `&` its typed id
  (`&CurrencyId`), a key struct with a text column as `&OrderItemKey`, and
  `search_after` takes `Option<&…>` for such keys. `Copy` keys stay by value.
- Query-file functions take text parameters as `&str` and array parameters
  as slices (`&[T]`), also inside `Option`. Parameter structs (more than
  three parameters) stay owned.
- Query-file functions use typed ids: a parameter typed by a primary key or
  foreign key column takes the table's typed id (`category_id: CategoryId`,
  `&CurrencyId` for a text key, `&[CategoryId]` for `= ANY($1)`), and a
  result column taken straight from such a column decodes as it. A
  parameter compared with two different ids, and computed expressions, keep
  the plain type.

### Breaking changes to the library

- `rok_sqlc::generate(&Config) -> Result<Report>` is the stable entry point.
  The `generator`, `output`, and `schema` modules are now private, and
  `cli`, `parser`, and `queries` are hidden from the documentation and not
  covered by semver.

### Added

- Models, reads (`get_by_id`, `exists_by_id`, `list`, `list_page`, `count`),
  and writes (`create`, `update`, `delete`) for every table in PostgreSQL
  DDL, with `@generated` overwrite protection and a schema snapshot.
- Composite primary keys, array columns, `TIME`, and `NUMERIC`.
- PostgreSQL enum types (`CREATE TYPE ... AS ENUM`, `ALTER TYPE ... ADD/RENAME
  VALUE`) become Rust enums in a generated `db::types` module, used by
  rows, inputs, filters, sort keys, and query files. `types` is a reserved
  module name.
- Value-list columns (text columns restricted by `CHECK (col IN ('a', ...))`,
  inline, as a table constraint, or added by `ALTER TABLE`) become Rust enums
  in `db::types`, named after table and column (`OrderStatus`), that encode
  as text and keep each exact value.
- Typed search: per-table `Filter` and `SortKey` types, `search`, and keyset
  pagination with `search_after`.
- Unique-key lookups: every unique constraint (inline, table-level, or added
  by `ALTER TABLE`) and unique index on plain columns gets
  `read::get_by_<col>[_and_<col>...]`, returning `Option<Row>`. Text key
  columns are borrowed (`&str`, or `&` a text typed id), other columns by
  value.
  Expression indexes and partial indexes get no lookup.
- Annotated query files (`-- name: X :one|:maybe_one|:many|:exec|:execrows`)
  generate typed query functions, including `WITH` (CTE) queries.
- A query file's `ON CONFLICT (cols)` target must be a unique key of the
  table (the primary key, a unique constraint, or a full unique index, in
  any column order); otherwise generation fails with a `path:line`
  diagnostic listing the table's unique keys. `ON CONFLICT ON CONSTRAINT
  name` must name the table's primary key or a unique constraint, by its
  declared name or PostgreSQL's default (`<table>_pkey`,
  `<table>_<columns>_key`); otherwise the diagnostic lists the valid names.
- `--schema` accepts files or migration directories and can be repeated;
  `ALTER TABLE` statements in later migrations change earlier tables.
  Diagnostics print `path:line:`. `ALTER TABLE … ADD CONSTRAINT … CHECK`
  that is not a value list is ignored, like such a `CHECK` in `CREATE
  TABLE`, instead of failing the table.
- `DROP TABLE` (also `IF EXISTS`, schema-qualified, or several names) in a
  later migration removes the table, so its generated files are stale and
  get removed. Foreign keys other tables hold to it are dropped as with
  `CASCADE`: the column keeps its raw type and makes no relationship.
  `DROP TYPE` removes an enum. With `CASCADE`, its columns are dropped too.
  Without it, tables still using the type fail.
- `--queries` accepts files or directories and can be repeated.
- `rok-sqlc.toml` discovery (or `-c/--config`) and a `-j/--json`
  configuration payload, layered as flags > payload > file > defaults.
- A `[conventions]` section in `rok-sqlc.toml` and the JSON payload
  (`rok_sqlc::Conventions` in the library), off by default, with a `rok`
  preset for the Rok ERP. Managed columns (`managed_columns`, such as
  `updated_at` and `row_version`) are left out of create inputs and update
  patches and kept in rows. See ADR 0010.
- Tenant scoping (`tenant_column`, `organization_id` in the `rok` preset). On
  a table with that column, every generated function takes the tenant id
  right after the executor and adds `<column> = <tenant>` to every statement:
  reads, unique-key lookups, `search`, `search_after`, `create`, `update`
  (versioned or not, empty patch included), and every delete. The column is
  left out of create inputs (it is set from the parameter) and update patches
  (it never changes). When it is part of the primary key, the tenant
  parameter replaces that key column, and unique-key lookups leave it out of
  their name (`UNIQUE (organization_id, code)` gives `get_by_code`).
- `db::session` (generated when `tenant_setting` or `user_setting` is set):
  `set_tenant(&mut tx, id)` and `set_user(&mut tx, id)` run a
  transaction-local `set_config`, for row-level security policies and audit
  triggers. `session` is a reserved module name.
- A version column (`version_column`, `row_version` in the `rok` preset) for
  optimistic concurrency. On a table that has it, `update` takes
  `expected_version` and returns `UpdateOutcome<Row>` (`Updated(row)`,
  `NotFound`, or `VersionConflict`), defined once in `db/mod.rs`. The check
  is part of the `UPDATE`'s condition, and one statement tells a stale
  version from a missing row. The version column is always managed. A
  soft-deleted row is `NotFound`. Tables
  without the column keep the plain `update`. See ADRs 0006 and 0010.
- Soft delete (`soft_delete_column`, `deleted_at` in the `rok` preset). On a
  table with that column, which must be a nullable `TIMESTAMP` or
  `TIMESTAMPTZ` (otherwise generation fails), `delete` sets it to `now()` on a
  live row and reports whether a row changed. `restore` clears it, and
  `delete_permanently` removes the row. Every generated read skips
  soft-deleted rows. The search filter gains `include_deleted: bool`, and
  `update` on a deleted row returns `None`. The column is left out of create
  inputs and update patches. A partial unique index whose condition is
  exactly `<column> IS NULL` now gets a `get_by_<cols>` lookup. Query files
  are not rewritten.
- Relationships inferred from foreign keys between the tables generated
  together (ADR 0014): a belongs-to per foreign key (`category_id` →
  `category`), its inverse has-many named after the child module
  (`products`, or `invoices_by_created_by_user` when the child has several
  foreign keys to the parent), a has-one when the foreign key column is a
  unique key, and `parent`/`children` for a `parent_id` self-reference. The
  tenant column's foreign key is not a relationship. Each resource with
  relationships gets `relationships.rs` with a `load_<name>` loader per
  relationship: it takes already-fetched rows and loads the relationship for
  all of them in one `= ANY($n)` query, returning a `HashMap` keyed by the
  joining key (typed id where there is one): one row per key for to-one,
  a list per key (empty when nothing relates) for to-many. Loaders on a
  tenant table take the tenant and skip soft-deleted rows. Two relationships
  of a table with the same name fail generation, naming both.
- Many-to-many relationships through join tables: a table whose two
  non-tenant `NOT NULL` foreign keys, to two other different tables, form a
  unique key (payload columns allowed) relates those tables many-to-many,
  named after the far table (`permissions`, or
  `permissions_through_role_permissions` when the name is taken or several
  join tables lead there). Its `load_<name>` loader goes through the join
  table in one query and respects both tables' tenant and soft-delete
  columns. The join table's `write.rs` gains `attach` (idempotent, returns
  whether the pair is new), `detach`, and `sync(near, &[far])`, which
  replaces the set in one statement, so it is atomic in the caller's
  transaction. On a soft-delete join table `detach` soft-deletes and
  `attach`/`sync` restore the soft-deleted row instead of inserting another.
  `attach` and `sync` are left out when another column needs a value. A
  table with two foreign keys but no unique pair (like `user_roles`) is not a
  join table. Opting out one side of a many-to-many removes the other;
  opting out a join table's foreign key removes the pair through it and
  its join writes. A many-to-many relationship filter goes through a live
  join row in the same tenant.
- Joined reads for to-one relationships (ADR 0014): a table with a primary
  key gets `get_with_<name>` in `relationships.rs` for each belongs-to and
  has-one, reading the row and its related row in one query into a
  `{Row}With{Relationship}` struct (`ProductWithCategory`). The related row
  is an `Option` unless a NOT NULL foreign key guarantees it (a has-one, a
  nullable foreign key, or a related table with a tenant or soft-delete
  column makes it optional). Columns are selected under per-side aliases, so
  shared column names decode unambiguously; the related table's tenant and
  soft-delete conditions sit in the join's `ON` clause, so a deleted or
  other-tenant parent reads as `None` instead of hiding the child.
- Relationship filters (ADR 0014): a resource's `{Struct}Filter` gains an
  `Option<Box<{Related}Filter>>` field per relationship
  (`ProductFilter { category: Some(Box::new(CategoryFilter { name, .. })), .. }`),
  rendered as a correlated `EXISTS` subquery so rows never repeat; for a
  to-many relationship it means "at least one related row matches". An
  empty related filter requires a related row (a `NULL` foreign key has
  none). Soft-deleted related rows never match, and a related tenant
  table's rows must share the searched row's tenant; an untenanted
  resource gets no filter field into a tenant table. Filters nest up to
  `db::search::MAX_RELATIONSHIP_DEPTH` (4) deep, and `search` fails with
  `sqlx::Error::InvalidArgument` beyond it. A relationship named like one of
  the filter's column fields fails generation, naming both; renaming it
  in `[relationships]` resolves the clash, and filters use renamed names
  and skip opted-out relationships.
- A `[relationships]` section (config file, JSON payload, or
  `Config::relationships` with `rok_sqlc::RelationshipChange`) renames or
  opts out inferred relationships, by table and inferred name:
  `[relationships.invoices]` with `created_by_user = "author"` generates
  `load_author`, and `approved_by_user = false` generates nothing for that
  foreign key on either side. Entries are checked against the whole schema,
  so `--resource` runs accept the same configuration; an entry naming an
  unknown table or relationship, an invalid name, or a rename that collides
  fails, naming the entry (`relationships.invoices.created_by_user`).
- `-k/--check` reports drift without writing and exits 1 when generated
  files are missing, changed, or stale. Successful runs remove stale
  generated files.
- Generation and `--check` ignore line endings: migrations and query files
  are read with CRLF normalized to LF (so the schema snapshot is the same on
  every platform), and generated files that differ only in CRLF/LF count as
  unchanged.
- Every generated public item is documented, and generated code passes
  `missing_docs` and `cargo clippy -- -D warnings` with a selected set of
  pedantic lints; both fixture crates enforce the bar (ADR 0013).
- `derive = ["serde"]` (config file, JSON payload, `-d/--derive serde`, or
  `Config::derive` with `rok_sqlc::Derive::Serde`) adds
  `serde::Serialize`/`Deserialize` to rows, typed ids (transparent), key
  structs, enums (by database label), inputs, patches, joined-read structs
  (`ProductWithCategory`), and query rows and parameters, and adds `serde` to the dependency hints. Without it,
  generated code does not mention `serde`.
- Short aliases for every command and flag; `--help` and `--version` exit 0.
- Crate metadata, license files, this changelog, and CI on Linux (Rust 1.88,
  1.94, and stable, with PostgreSQL), macOS, and Windows.

### Changed

- The minimum supported Rust version is 1.88 (`rust-version = "1.88"`).
  Generated code needs Rust 1.94 (SQLx 0.9), so on toolchains below 1.94
  the test suites that compile generated code print a skip notice instead
  of running.
- Usage errors print clap's message once and exit 1.
- Diagnostics and file lists print paths with `/` on every platform, and
  paths under the current directory relative to it (also when they come
  from `rok-sqlc.toml`).

[Unreleased]: https://github.com/ateeq1999/rok-sqlc/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/ateeq1999/rok-sqlc/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/ateeq1999/rok-sqlc/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/ateeq1999/rok-sqlc/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ateeq1999/rok-sqlc/releases/tag/v0.1.0
