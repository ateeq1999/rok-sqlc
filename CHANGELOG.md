# Changelog

All notable changes to rok-sqlc are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Changes to
the *generated* API count as breaking (see ADR 0012).

## [Unreleased]

Nothing has been published to crates.io yet. The entries below record the
work toward 0.1.0, including generated-API changes that break code written
against earlier development snapshots.

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
  diagnostic listing the table's unique keys.
- `--schema` accepts files or migration directories and can be repeated;
  `ALTER TABLE` statements in later migrations change earlier tables.
  Diagnostics print `path:line:`.
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
- `-k/--check` reports drift without writing and exits 1 when generated
  files are missing, changed, or stale. Successful runs remove stale
  generated files.
- Every generated public item is documented, and generated code passes
  `missing_docs` and `cargo clippy -- -D warnings` with a selected set of
  pedantic lints; both fixture crates enforce the bar (ADR 0013).
- `derive = ["serde"]` (config file, JSON payload, `-d/--derive serde`, or
  `Config::derive` with `rok_sqlc::Derive::Serde`) adds
  `serde::Serialize`/`Deserialize` to rows, typed ids (transparent), key
  structs, enums (by database label), inputs, patches, and query rows and
  parameters, and adds `serde` to the dependency hints. Without it,
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
