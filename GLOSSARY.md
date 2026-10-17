# rok-sqlc

A tool that turns SQL a developer writes (schema migrations and annotated queries) into SQLx-based Rust code for querying PostgreSQL.

## Language

**Table**:
A table declared by `CREATE TABLE` in the input schema; the input side of generation.
_Avoid_: entity, resource (when referring to the SQL side)

**Resource**:
The Rust module rok-sqlc generates for one table; the output side of generation.
_Avoid_: table (when referring to generated output)

**Schema snapshot**:
A verbatim copy of the input schema SQL embedded in generated code as a constant. Never executed; not a migration system.
_Avoid_: migration

**Module fixture**:
An ERP schema and query set under `modules/` used only as test input; never a layout users are expected to follow.
_Avoid_: test schema, sample

**Query file**:
A file of annotated SQL statements a developer writes; each annotated statement becomes one generated function.
_Avoid_: schema (for DML content), script

**Return mode**:
The `:one`, `:maybe_one`, `:many`, `:exec`, or `:execrows` suffix of a query annotation, choosing how rows are fetched and what the generated function returns.
_Avoid_: command, kind

**Query override**:
A `-- param: $N name Type` or `-- column: name Type` line that declares a type the generator cannot infer from the SQL and schema.
_Avoid_: hint, annotation (reserved for `-- name:`)

**Migration**:
One schema file a developer writes; taken in order, a project's migrations describe its tables.
_Avoid_: schema file (when several are meant)

**Config file**:
An optional `rok-sqlc.toml` holding the same options as the CLI, found in the current directory or a parent (or named with `-c`); its paths are relative to the file, and the JSON payload and flags override it.
_Avoid_: settings, manifest

**JSON payload**:
Configuration passed to `-j/--json` as JSON in the same shape as the config file: inline, `@path` for a file, or `-` for stdin. Its paths are relative to the current directory; it overrides the config file, and flags override it.
_Avoid_: JSON config, JSON file (it need not be a file)

**Convention**:
An opt-in rule about what certain columns mean across tables (for example, which column scopes rows to a tenant), changing what is generated for every table that has the column.
_Avoid_: rule, policy (reserved for PostgreSQL row-level security)

**Preset**:
A named bundle of conventions, such as the conventions of the Rok ERP.
_Avoid_: profile, template

**Tenant column**:
The column that says which tenant (organization) a row belongs to; under the tenant convention, every generated operation on such a table is scoped to one tenant given by the caller.
_Avoid_: owner, account

**Soft delete**:
Marking a row deleted by recording when it was deleted, instead of removing it; soft-deleted rows are invisible to generated reads unless asked for, and can be restored.
_Avoid_: archive, trash

**Permanent delete**:
Removing a row from the table, as opposed to a soft delete.
_Avoid_: hard delete, purge

**Unique key**:
A set of columns no two live rows share (the primary key, a unique constraint, or an eligible unique index); each one gets a generated lookup.
_Avoid_: natural key, alternate key

**Drift**:
A difference between the generated files on disk and what the current SQL would generate.
_Avoid_: out of sync, dirty

**Stale file**:
A generated file whose source no longer exists (for example, a dropped table); generation removes it. A table that still exists but is skipped as unsupported is not a stale source: its files are kept (when rok-sqlc knows its last good shape; see ADR 0001).
_Avoid_: orphan, leftover

**Last good shape**:
A skipped table's model just before the statement that made it unsupported. Its kept files were generated from it, so the rest of a lenient run is planned against it (ADR 0001).
_Avoid_: snapshot (that is the schema snapshot)

**Typed id**:
A distinct Rust type for one table's key, used wherever that key appears, so keys of different tables cannot be mixed up.
_Avoid_: id wrapper, newtype (in user-facing text)

**Key struct**:
A struct naming each column of a composite primary key, taken by lookups and keyset pagination in place of separate parameters.
_Avoid_: key tuple, compound id

**Managed column**:
A column the database maintains itself on write (for example, a last-updated time); callers never supply it.
_Avoid_: system column, audit column

**Version column**:
A column the database advances on every update, letting an update proceed only when the row is still at the version the caller last saw (optimistic concurrency).
_Avoid_: revision, lock column

**Enum type**:
A PostgreSQL type declared by `CREATE TYPE … AS ENUM` (and changed by `ALTER TYPE … ADD VALUE`, `RENAME VALUE`, and `RENAME TO`), generated as a Rust enum in the shared `types` module. Each variant keeps its exact database **label**.
_Avoid_: enumeration, domain

**Value-list column**:
A text column restricted to a fixed list of values by `CHECK (col IN ('a', 'b', ...))`, inline or as a table constraint. Generated as a Rust enum in the shared `types` module, named after its table and column (`products.product_type` gives `ProductProductType`), that encodes as text and keeps each exact value. Otherwise treated like an **Enum type**.
_Avoid_: check enum, status string

## Writes

**Generated column**:
A column whose value the database always owns: `SERIAL`, identity, or `GENERATED`. Never present in a create input or update patch.
_Avoid_: auto column, computed (for identity/serial)

**Defaulted column**:
A column the database can supply a value for (`DEFAULT`, serial, identity, or `GENERATED`), but which a caller may still set explicitly.
_Avoid_: optional column

**Create input**:
The `NewXxx` struct a caller passes to `create` (`Xxx` is the singular row struct name, e.g. `NewProduct` for `products`); generated columns are omitted and defaulted columns are optional, so an omitted value lets the database default apply.
_Avoid_: insert model, DTO

**Update patch**:
The `UpdateXxx` struct a caller passes to `update`; each field is optional, where `None` means "leave unchanged" and, for a nullable column, `Some(None)` means "set NULL".
_Avoid_: update model, diff

## Execution

**Filter**:
The generated `XFilter` struct a caller passes to `search`/`search_after`: one list of typed conditions per filterable column, all combined with `AND`, plus `x_is_null` checks for nullable columns.
_Avoid_: where clause, query (for this struct)

**Sort key**:
A variant of the generated `XSortKey` enum: the allowlist of columns `search` may order by.
_Avoid_: order-by string, sort column

**Keyset pagination**:
Paging by "rows after this primary key" (`search_after`) instead of by offset, so pages stay correct while rows are inserted or deleted.
_Avoid_: cursor (when it means an offset)

**Executor**:
The generic `sqlx::PgExecutor` a generated CRUD function runs against; a caller passes `&pool` or `&mut *tx`, keeping the transaction boundary theirs.
_Avoid_: pool (when it may be a transaction), connection

## Relationships

**Relationship**:
An association between two tables that generation may traverse to produce baseline join reads.
_Avoid_: join (the SQL mechanic), link

**Belongs-to**:
A relationship from the table holding the foreign key to the table it references.
_Avoid_: reference, parent

**Has-many**:
A relationship from a referenced table to the tables whose foreign keys point at it.
_Avoid_: children

**Has-one**:
A has-many variant where the foreign key carries a uniqueness constraint.
_Avoid_: single child

**Through relationship**:
A two-hop relationship traversing an intermediate table (has-many-through, has-one-through).
_Avoid_: nested

**Many-to-many**:
Two has-many-through relationships over a shared join table.
_Avoid_: m2m

**Join table**:
A table whose two (non-tenant) foreign keys together form a unique key, implementing a many-to-many; it may carry columns of its own.
_Avoid_: pivot, junction

**Polymorphic association**:
A declared (never inferred) association whose foreign key and type discriminator may point at one of several tables.
_Avoid_: FK (for this construct)

**Loader**:
A generated function that fetches one relationship for many already-fetched rows in a single round trip ("with" loading), instead of one query per row.
_Avoid_: eager load (for the function itself), include

**Relationship filter**:
A condition on a related table that decides which rows match, without adding the related rows to the result.
_Avoid_: join filter, nested where
