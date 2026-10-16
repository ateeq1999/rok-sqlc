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
- Annotated query files (`-- name: X :one|:maybe_one|:many|:exec|:execrows`)
  generate typed query functions, including `WITH` (CTE) queries.
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
  patches and kept in rows. The tenant, soft-delete, and version keys are
  validated but not applied yet. See ADR 0010.
- `-k/--check` reports drift without writing and exits 1 when generated
  files are missing, changed, or stale. Successful runs remove stale
  generated files.
- Short aliases for every command and flag; `--help` and `--version` exit 0.
- Crate metadata, license files, this changelog, and CI on Linux (Rust 1.88,
  1.94, and stable, with PostgreSQL), macOS, and Windows.

### Changed

- The minimum supported Rust version is 1.88 (`rust-version = "1.88"`).
  Generated code needs Rust 1.94 (SQLx 0.9), so on toolchains below 1.94
  the test suites that compile generated code print a skip notice instead
  of running.
- Usage errors print clap's message once and exit 1.
