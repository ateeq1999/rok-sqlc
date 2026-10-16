# Spec: rok-sqlc 0.1, from "it generates code" to a production-ready crate

Status: ready-for-agent

## Problem Statement

A developer who keeps a PostgreSQL schema as migration files wants to write SQL and get typed, idiomatic Rust code that queries the database with SQLx. rok-sqlc already generates CRUD per table, typed search, and functions for annotated query files, but it is not yet a tool such a developer can adopt.

- It accepts a single schema file, while real projects keep ordered folders of migrations. It ignores `ALTER TABLE`, so a migration that adds a column silently produces a model without it.
- It has no configuration file, no way to be driven from scripts or build scripts, and no way to check in CI that generated code is current. `--help` and `--version` exit with an error. Errors name a table but not the file or line.
- Real schemas use enumerated types and value-list columns, unique keys, soft deletes, multi-tenant scoping with row-level security, managed columns, and optimistic concurrency. The generator understands none of these. Upserts against non-existent unique keys pass generation and fail at runtime.
- Relationships between tables, which are the most common thing application code reaches for, have to be written by hand, and loading related rows tends to become one query per row.
- Generated keys are bare `Uuid`/`i64` values that can be passed to the wrong table's functions.
- The crate has no CI, no license files, no changelog, no declared minimum Rust version, and no stable library API. Its README is a design walkthrough for building the tool, not a guide to using it.

## Solution

rok-sqlc 0.1 is a public crate (`cargo install rok-sqlc`) that turns a project's **migrations** and **query files** into SQLx 0.9 Rust code. It is general-purpose, and the Rok ERP's conventions ship as an opt-in **preset**.

- **Inputs and configuration.** Migrations and query files can be given as files or directories, and each flag can be repeated. Options can come from flags, a JSON payload, or an optional `rok-sqlc.toml`. Every command and flag has a short alias. Errors cite `path:line`. Generation removes **stale files**, and a check mode reports **drift** for CI. Build scripts use one stable library entry point.
- **Schema understanding.** Shape-changing `ALTER TABLE`/`ALTER TYPE` statements are applied, harmless statements are ignored on purpose, and anything else that targets a known table fails. Enumerated types and **value-list columns** become Rust enums. **Unique keys** get lookups, and `ON CONFLICT` targets are checked against them.
- **Idiomatic generated code.** Every table's key is a **typed id**, and composite keys are key structs. Every public item is documented, and generated code passes rustfmt, clippy, and `missing_docs`. Parameters are borrowed where that is natural. `serde` derives are available on request.
- **Conventions** (opt-in, all on in the `rok` preset):
  - **Soft delete**, with restore and **permanent delete**.
  - **Tenant column** scoping by explicit parameter, plus a session helper for row-level security.
  - **Managed columns**.
  - A **version column** for optimistic concurrency.
- **Relationships.** Relationships are inferred from foreign keys:
  - **Loaders** fetch "with" relationships, one query per relationship.
  - Joined reads cover to-one relationships.
  - **Relationship filters** inside typed search.
  - Many-to-many relationships get attach, detach, and sync.
  - Relationships can be renamed or opted out in configuration.
- **Query files** gain `WITH` (CTE) support.
- **Release readiness.** CI on Linux, macOS, and Windows, including a PostgreSQL suite. A **module fixture** harness generates, compiles, and runs the whole 25-module ERP corpus. License files, crates.io metadata, a changelog, `rust-version = "1.88"`, and a rewritten user-facing README.

## User Stories

### Inputs, configuration, and CLI

1. As a developer, I want to point `--schema` at my migrations directory, so that I don't have to concatenate files before generating.
2. As a developer, I want down migrations skipped automatically, so that reverse migrations never pollute my models.
3. As a developer, I want migrations read in filename order, so that later migrations build on earlier ones exactly as they do when applied.
4. As a developer, I want to repeat `--schema` to combine several files or directories, so that shared and module-specific migrations can be generated together.
5. As a developer, I want to point `--queries` at a directory of query files and get one generated Rust module per file, so that my queries stay organized the way I wrote them.
6. As a developer, I want a single query file to keep producing one queries module, so that existing setups keep working.
7. As a developer, I want an optional `rok-sqlc.toml` holding the same options as the CLI, so that the team runs one command with no flags.
8. As a developer, I want `rok-sqlc.toml` discovered from the current directory upwards, so that I can run the tool from any subdirectory like Cargo.
9. As a developer, I want paths in `rok-sqlc.toml` resolved relative to the file, so that the config means the same thing wherever I run it.
10. As a developer, I want to pass configuration as JSON with `-j/--json` (inline, `@file`, or `-` for stdin), so that scripts, editors, and other tools can drive rok-sqlc.
11. As a developer, I want flags to override the JSON payload, which overrides `rok-sqlc.toml`, which overrides defaults, so that the most specific source always wins.
12. As a developer, I want unknown keys in a JSON payload or config file to be errors, so that a typo can't silently do nothing.
13. As a developer, I want every command and flag to have a short alias (`g`, `-s`, `-o`, `-r`, `-q`, `-j`, `-c`, `-k`, `-n`, `-f`, `-l`, `-h`, `-V`), so that daily use is fast.
14. As a maintainer, I want a test that fails when any flag lacks a short alias or two aliases collide, so that the alias rule can't erode.
15. As a developer, I want `--help` and `--version` to print and exit successfully, so that scripts and CI checks don't see them as failures.
16. As a developer, I want every table and query failure to cite `path:line`, so that I can jump straight to the problem in a directory of migrations.
17. As a developer, I want generation to remove generated files whose source no longer exists (**stale files**), so that a dropped table doesn't leave dead code behind.
18. As a developer, I want generation never to touch files without the rok-sqlc header, so that my hand-written code is safe.
19. As a developer, I want `generate --check` to write nothing and exit non-zero when generated code is missing, changed, or stale, so that CI catches **drift**.
20. As a developer, I want `--check` to list exactly which files drifted, so that I know what regenerating will change.
21. As a developer, I want a stable `rok_sqlc::generate(&Config)` library entry point returning a report, so that I can regenerate from `build.rs` with the same behavior as the CLI.
22. As a developer, I want the library's internals hidden and unstable, so that the crate can evolve without breaking my build script.

### Schema understanding

23. As a developer, I want `ALTER TABLE … ADD COLUMN`, `DROP COLUMN`, `RENAME COLUMN`, and `ADD CONSTRAINT` applied to my models, so that generated code matches what my migrations actually produce.
24. As a developer, I want row-level security, policies, indexes, grants, triggers, and functions to be recognized and ignored on purpose, so that ordinary migrations generate cleanly.
25. As a developer, I want any other `ALTER TABLE` on a known table to fail with a diagnostic, so that an unsupported change can never silently produce a wrong model.
26. As a developer, I want foreign keys added by `ALTER TABLE … ADD CONSTRAINT` to count like inline ones, so that relationships don't depend on where a constraint was declared.
27. As a developer, I want `CREATE TYPE … AS ENUM` to become a Rust enum that keeps each exact database label, so that mixed-case labels round-trip safely.
28. As a developer, I want `ALTER TYPE … ADD VALUE` applied, so that enums evolve with my migrations.
29. As a developer, I want schema-qualified enum types to keep their schema, so that enums in different schemas never clash.
30. As a developer, I want text columns with `CHECK (col IN ('a', 'b', …))` to become Rust enums (**value-list columns**), so that statuses and kinds are typed instead of stringly typed.
31. As a developer, I want enum and value-list columns to be filterable, sortable, and usable in arrays, so that they behave like any other column.
32. As a developer, I want a `get_by_<columns>` lookup for every **unique key** (a unique constraint, or a unique index on plain columns), so that I can fetch by SKU or code without writing SQL.
33. As a developer, I want partial unique indexes whose condition is exactly the soft-delete condition to count as unique keys, so that "unique among live rows" lookups work.
34. As a developer, I want other partial indexes and expression indexes skipped, so that no lookup promises uniqueness the database doesn't enforce.
35. As a developer, I want an `ON CONFLICT` target that matches no unique key to fail generation and name the keys that do exist, so that an upsert can't pass generation and fail at runtime.
36. As a developer, I want foreign keys to schema-qualified tables to resolve to exactly that table, so that same-named tables in different schemas are never confused.

### Idiomatic generated code

37. As a developer, I want every table's key to have its own **typed id** (for example `ProductId`), used by the row, by every foreign key pointing at it, and by every function, so that passing the wrong table's key is a compile error.
38. As a developer, I want typed ids to be `Copy`, hashable, displayable, and convertible from and into the underlying value, so that they are as easy to use as the raw value.
39. As a developer, I want composite keys to be key structs instead of tuples, so that call sites name each part.
40. As a developer, I want every public generated item documented, and generated code clean under rustfmt, clippy (including selected pedantic lints), and `missing_docs`, so that it meets the bar of hand-written library code.
41. As a developer, I want `Copy` keys taken by value and lookups by borrowed text or slices where natural, so that calling generated code needs no needless clones.
42. As a developer, I want `serde` derives available through configuration, so that rows can go straight into API responses.
43. As a developer, I want a breaking change to generated APIs treated as a breaking crate release, recorded in the changelog, so that upgrades are predictable.

### Conventions

44. As a developer, I want conventions to be opt-in and off by default, so that rok-sqlc fits schemas that don't follow them.
45. As a developer, I want a `rok` **preset** that turns on the Rok ERP's conventions in one line, so that Rok modules need no per-convention setup.
46. As a developer, I want `delete` on a soft-delete table to record the deletion time on rows not already deleted, and report whether a row changed, so that deletes are reversible.
47. As a developer, I want `restore` and `delete_permanently`, so that I can undo a soft delete or really remove a row.
48. As a developer, I want every generated read (`get_by_id`, unique-key lookups, `exists_by_id`, `list`, `list_page`, `count`, `search`, `search_after`, loaders) to skip soft-deleted rows, so that deleted data never leaks by accident.
49. As a developer, I want an `include_deleted` switch on the search filter, so that admin screens can still see deleted rows.
50. As a developer, I want `update` on a soft-deleted row to report nothing to update, so that deleted rows can't be modified by accident.
51. As a developer, I want a soft-delete column of the wrong type to fail generation, so that the convention never applies to the wrong column.
52. As a developer, I want query files never rewritten by conventions, so that my explicit SQL runs exactly as written.
53. As a developer, I want every generated function on a tenant table to take the tenant id explicitly and scope every statement to it, so that rows can't leak across tenants even without row-level security.
54. As a developer, I want the tenant column removed from create inputs and update patches, so that a row's tenant comes only from the scope and can never be changed.
55. As a developer, I want the tenant parameter to replace the matching key column when the tenant column is part of the primary key, so that I never pass the same value twice.
56. As a developer, I want optional `db::session` helpers that set the tenant and the acting user for the current transaction (`set_config(…, true)`), so that row-level security policies and audit triggers see them.
57. As a developer, I want **managed columns** (such as `updated_at` and `row_version`) excluded from inputs and patches, so that I don't fight database triggers.
58. As a developer, I want `update` on a table with a **version column** to take the version I last saw and return `Updated`, `NotFound`, or `VersionConflict`, so that concurrent edits are detected instead of overwritten.

### Relationships

59. As a developer, I want every foreign key to give a belongs-to relationship named after the column without `_id`, so that relationships read naturally (`category_id` → `category`).
60. As a developer, I want foreign keys to unique non-key columns to count, so that references to codes and keys are relationships too.
61. As a developer, I want every belongs-to to give the inverse has-many, named after the child, so that I can navigate both ways.
62. As a developer, I want inverse names disambiguated when a child has several foreign keys to the same parent (`invoices_by_created_by_user`), so that names never collide.
63. As a developer, I want self-references through `parent_id` to become `parent` and `children`, so that trees read naturally.
64. As a developer, I want a foreign key on a unique column to become has-one, so that one-to-one relationships return one row.
65. As a developer, I want a **join table** (two non-tenant foreign keys that together form a unique key, payload columns allowed) to become a many-to-many relationship, so that roles ↔ permissions works without configuration.
66. As a developer, I want the tenant column's foreign key not treated as a relationship, so that every table doesn't sprout an "organization" relationship.
67. As a developer, I want **loaders** that take already-fetched rows and load one relationship for all of them in a single query, so that "with" loading never becomes one query per row.
68. As a developer, I want loaders to return maps keyed by typed ids (one parent per key for to-one, a list per key for to-many), so that attaching related rows is a lookup.
69. As a developer, I want a single-query joined read for to-one relationships (for example `get_with_author`), so that the common "row plus its parent" case is one round trip.
70. As a developer, I want **relationship filters** in typed search (for example products whose category's name matches), rendered without duplicating rows, so that I can filter by related data without writing SQL.
71. As a developer, I want `attach`, `detach`, and `sync` for many-to-many relationships, with `sync` running inside my transaction, so that I can manage join rows safely.
72. As a developer, I want `detach` and `attach` on a soft-delete join table to soft-delete and re-activate rows, so that join history follows the convention.
73. As a developer, I want loaders, joined reads, and relationship filters to respect the soft-delete and tenant conventions, so that relationships never leak deleted or foreign rows.
74. As a developer, I want to rename an inferred relationship or opt it out in configuration, so that I can fix names inference gets wrong.
75. As a developer, I want nested writes left to me with ordinary `create` calls on one transaction, so that the generated surface stays small and explicit.

### Query files

76. As a developer, I want `WITH` (CTE) queries in query files to be analyzed and typed like any other query, so that complex reads don't need overrides.
77. As a developer, I want a CTE's result columns resolvable by the main statement, so that types flow through the CTE.

### Release readiness

78. As a contributor, I want CI to run formatting, clippy (`-D warnings`), all tests, and the PostgreSQL suite against a service container on Linux, and build checks on macOS and Windows, so that every change is verified the same way.
79. As a contributor, I want CI to test both the minimum supported Rust version (1.88) and stable, so that the declared minimum stays true.
80. As a maintainer, I want a module fixture harness that generates every fixture module into a fixture crate, compiles it, checks formatting, and runs it against migrated PostgreSQL, so that the real-world corpus guards every change.
81. As a user, I want license files, crates.io metadata, and a changelog, so that I can evaluate and adopt the crate with confidence.
82. As a user, I want a README that shows SQL in and Rust out, then install, quickstart, the generated API, query files, conventions, configuration, `build.rs` use, CI use, supported SQL, limitations, and how to contribute, so that I can adopt rok-sqlc without reading the source.
83. As a contributor, I want the old design walkthrough preserved in `docs/design.md`, so that the project's history stays available without cluttering the README.

## Implementation Decisions

### Inputs, configuration, CLI

- One configuration model is shared by the CLI, the JSON payload, `rok-sqlc.toml`, and the library entry point. Sources merge with flags > JSON payload > `rok-sqlc.toml` > defaults. Config-file paths resolve relative to the file; JSON-payload paths resolve relative to the current directory. Unknown keys are errors.
- Config shape (illustrative; keys may grow, not change meaning):

  ```toml
  schema  = ["migrations/"]
  queries = ["queries/"]
  output  = "src/db"
  derive  = ["serde"]            # optional

  [conventions]                  # all optional, all off by default
  preset             = "rok"
  tenant_column      = "organization_id"
  tenant_setting     = "app.organization_id"
  user_setting       = "app.user_id"
  soft_delete_column = "deleted_at"
  managed_columns    = ["updated_at", "row_version"]
  version_column     = "row_version"

  [relationships]                # rename / opt-out inferred relationships
  ```
- The `rok` preset expands to the conventions above. Explicit keys override preset values.
- Schema inputs: a file, or a directory read in filename order with `*.down.sql` skipped. Repeatable. Every parsed statement keeps its source path and line for diagnostics.
- Query inputs: a file produces the queries module; a directory produces one module per file, named after the file stem. Repeatable.
- `--help`/`--version` print and exit 0.
- Aliases: `generate`→`g`, `--schema`→`-s`, `--output`→`-o`, `--resource`→`-r`, `--queries`→`-q`, `--json`→`-j`, `--config`→`-c`, `--check`→`-k`, `--dry-run`→`-n`, `--force`→`-f`, `--lenient`→`-l`, `--help`→`-h`, `--version`→`-V`. New commands and flags must add an unused alias.
- Stale files: after a successful run, files under the output directory that carry the generated header but are not in the plan are removed. `--check` computes missing, changed, and stale files, writes nothing, and exits 1 on any drift.
- Library: one stable `generate(&Config) -> Result<Report>` entry point (and the config types it takes). Everything else is hidden or crate-private.
- Generated code targets SQLx 0.9 only. The tool declares `rust-version = "1.88"`; generated code's lower floor is Rust 1.94 (SQLx 0.9's MSRV) and is documented separately.

### Schema understanding

- ALTER handling: apply `ADD COLUMN`, `DROP COLUMN`, `RENAME COLUMN`, and `ADD CONSTRAINT` (primary key, unique, foreign key). Explicitly ignore row-level security, policies, indexes (except unique indexes, which feed unique keys), grants, triggers, comments, and function definitions or calls. Fail on any other `ALTER TABLE` targeting a known table.
- Enums: `CREATE TYPE … AS ENUM` and `ALTER TYPE … ADD VALUE` produce a shared generated types module. Each variant keeps its exact label via a per-variant rename, and the type name keeps its schema. Value-list columns (`CHECK (col IN (…))` on text) produce enums the same way. Enum columns filter with equality conditions and are sort keys. Arrays of enums are supported.
- Unique keys: the primary key, unique constraints, and unique indexes on plain columns. Partial indexes count only when their condition equals the soft-delete condition. Expression indexes and other partial indexes are skipped. Each unique key except the primary key gets `get_by_<cols>`. `ON CONFLICT` targets must match a unique key.
- Foreign keys record the referenced schema, table, and column(s).
- Reserved generated module names grow to cover `schema`, `search`, `types`, `session`, and `queries`.

### Idiomatic generated code

- Typed ids per table key: transparent SQLx type, `Copy`/`Clone`/`Eq`/`Hash`/`Ord`/`Debug`/`Display`, conversions to and from the inner value. Foreign key fields use the referenced table's typed id. Composite keys get key structs, used by lookups and `search_after` cursors.
- Generated code must pass rustfmt, clippy with `-D warnings`, a selected pedantic set, and `missing_docs`. The fixture crates enforce this.
- Parameters: `Copy` values by value; text keys and lists borrowed where the call doesn't need ownership. Inputs and patches stay owned.
- Optional `serde` derives through `derive = ["serde"]`.
- Generated types are not `#[non_exhaustive]`: the user owns the schema.

### Conventions

- Soft delete (nullable timestamp column, otherwise a diagnostic):
  - `delete` sets the timestamp where it is null and reports whether a row changed. `restore` clears it. `delete_permanently` removes the row.
  - All generated reads and loaders exclude soft-deleted rows; the search filter has `include_deleted`. `update` on a deleted row reports not found.
  - The column is excluded from inputs and patches. Query files are untouched.
- Tenant: functions on tenant tables take the tenant id right after the executor and add the tenant condition to every statement, including joins, loaders, and relationship filters. The tenant column is excluded from inputs and patches. When it is part of the primary key, the tenant parameter replaces that key column. Optional session helpers set the tenant and user settings transaction-locally.
- Managed columns are excluded from inputs and patches.
- Version column: `update` takes the expected version and returns an outcome of updated (with the row), not found, or version conflict. The version check is part of the update's condition.

### Relationships

- Inference rules:
  - Belongs-to for every foreign key, excluding the tenant column's.
  - Has-many as the inverse.
  - Has-one when the foreign key column is a unique key.
  - Many-to-many for join tables.
  - Naming from the foreign key column without `_id`, with the child-named inverse disambiguated as `{child}_by_{relation}`, and `parent`/`children` for `parent_id` self-references.
- Configuration can rename or opt out any inferred relationship. Declared has-many-through and polymorphic relationships are out of scope (ADR 0002's declared lane, beyond rename and opt-out, arrives later).
- Loaders take a slice of rows and issue one query using `= ANY(…)`. To-one loaders return a map from the related key to the related row; to-many loaders return a map from the parent key to a list.
- Joined reads exist for to-one relationships only, returning a struct holding both rows; a nullable foreign key yields an optional related row.
- Relationship filters add an optional related-table filter to a table's search filter, rendered as an `EXISTS` subquery so rows are never duplicated.
- Many-to-many mutations: `attach` (idempotent), `detach`, and `sync` (replace the set within the caller's transaction). These respect soft delete on the join table.
- Ad-hoc multi-table reads remain query-file territory.

### Query files

- `WITH` clauses: each CTE's result columns are inferred like a `SELECT` and become a scope the main statement can reference. Recursive CTEs are rejected with a diagnostic.

### Release

- CI: fmt, clippy `-D warnings`, tests including the PostgreSQL suite (service container), on Linux with Rust 1.88 and stable; build and test on macOS and Windows without PostgreSQL.
- License files for MIT and Apache-2.0; crates.io metadata (repository, keywords, categories, readme); `CHANGELOG.md`.
- The module fixture harness replaces the ad-hoc corpus check: it generates every fixture module into a fixture crate under `db/modules/<module>/`, compiles it with the lint gate, checks formatting, applies the migrations to PostgreSQL in dependency order, and smoke-runs reads and search for every table.
- ADRs land with their implementations:
  - 0010 conventions.
  - 0011 inputs, configuration, and CLI.
  - 0012 library API and stability.
  - 0013 typed ids and the generated-code bar.
  - 0014 relationships (`with` and `join`).

## Testing Decisions

- A good test exercises behavior through a public seam and asserts outcomes a user would observe: generated files and CLI exit codes, or rows in a real database. It never asserts the generator's internal structure. Expected values come from independent sources (literals, worked examples), never recomputed the way the code computes them.
- Seams, highest first:
  1. **PostgreSQL integration**: the fixture crate's integration tests call generated functions against a real database (opt-in via `ROK_SQLC_TEST_DATABASE_URL`). This is the primary seam for conventions, relationships, unique keys, enums, typed ids, and query files. Prior art: the existing PostgreSQL scenarios, which use test-first slices and mutation checks to prove sensitivity.
  2. **Module fixture harness**: the whole ERP corpus generated, compiled, formatted, and smoke-run.
  3. **CLI**: invoking the CLI entry point and the binary, then asserting on files, output, and exit codes. This covers configuration precedence, JSON payload, directory inputs, aliases, `--check`, stale files, diagnostics with `path:line`, and `--help`/`--version`. Prior art: the existing CLI tests.
  4. **Parser diagnostics**: parser-level unit tests for rejections that no runtime seam can observe (ALTER failures, name collisions, unsupported types). Prior art: the existing parser tests.
- The existing gates stay on: generated code compiles under `deny(warnings)` (growing to clippy and `missing_docs`) and is rustfmt-clean.
- Renderer-text unit tests are not added for new features; behavior is proven at the seams above.

## Out of Scope

- Full-text search, pgvector, and embedding persistence.
- Compile-time-checked `query!` mode.
- Declared has-many-through and polymorphic relationships (beyond renaming and opting out inferred ones).
- Nested writes (creating a row together with its children).
- Machine-readable output (`--format json`).
- Prebuilt binaries (cargo-dist).
- SQLx versions other than 0.9; a user-facing module-directory input layout (module fixtures stay test-only); databases other than PostgreSQL.
- JSONB operators in typed filters.

## Further Notes

- Corpus facts that drove the design:
  - 175 tenant columns, with row-level security enabled by a shared setup function on every tenant table.
  - 167 soft-delete columns, with unique indexes scoped to live rows.
  - A trigger that maintains `updated_at` and increments `row_version`.
  - 626 single-column foreign keys, 352 of them cross-schema.
  - Join tables with surrogate keys and payload columns.
  - 330 `CHECK` constraints, many of them value lists.
- Delivery order (each a vertical slice with its ADR):
  1. Inputs and configuration.
  2. ALTER handling.
  3. Enums.
  4. Typed ids and the idiomatic bar.
  5. Unique keys.
  6. Conventions.
  7. Relationships.
  8. CTEs.
  9. Corpus harness, CI, and release metadata.
  10. README.
- Terminology follows `GLOSSARY.md`: permanent delete (not "hard delete"), typed id, loader, relationship filter, value-list column, managed column, version column, drift, stale file.
