# Conventions: opt-in column meanings, a `rok` preset, and annotations on the schema model

A **convention** says what a column means in every table that has it: which column scopes rows to a tenant, which one records a soft delete, which ones the database maintains, which one versions rows. rok-sqlc is general-purpose, so every convention is **off by default**: without configuration, a column named `deleted_at` or `updated_at` is an ordinary column and generated code is byte-for-byte what it was before conventions existed.

```toml
[conventions]                   # all optional, all off by default
preset             = "rok"
tenant_column      = "organization_id"
tenant_setting     = "app.organization_id"
user_setting       = "app.user_id"
soft_delete_column = "deleted_at"
managed_columns    = ["updated_at", "row_version"]
version_column     = "row_version"
```

**The section lives in `rok-sqlc.toml` and the JSON payload** (`{"conventions": {...}}`), like every other option (ADR 0011). There are no flags for it. Keys merge **one by one** across sources (payload over file), then the **preset expands**: its values are the base and every explicit key overrides one of them. A list (`managed_columns`) replaces the preset's list whole. An empty string turns a preset's single-column convention off (`version_column = ""`), and `managed_columns = []` manages nothing but the version column. Unknown keys and unknown presets are errors that name the source and list the valid choices.

**The `rok` preset** is the Rok ERP's conventions, exactly the values above. It is the only preset. The library exposes the result as `rok_sqlc::Conventions` (`Config::conventions`; `Conventions::rok()` and `Conventions::preset(name)`); like `Config` it is `#[non_exhaustive]`, so later conventions are not breaking.

**A convention applies per table, by column name**: only tables that have the named column are affected. Query files are never rewritten by conventions; explicit SQL runs exactly as written.

**Managed columns** (the database maintains them with defaults and triggers, such as a `touch_row` trigger setting `updated_at` and bumping `row_version`) are left out of create inputs and update patches but stay in rows, so `RETURNING` shows the values the database chose. A managed column with neither a default nor an insert trigger can't be inserted: that is the schema's contract, and rok-sqlc does not second-guess it.

**The version column** is always managed, whether or not `managed_columns` lists it: the database advances it (a `touch_row`-style trigger), and a caller who could write it could defeat the check. On a table that has it, `update` takes `expected_version` and returns `UpdateOutcome<Row>`: `Updated(row)`, `NotFound`, or `VersionConflict`. The version check is part of the `UPDATE`'s condition, and the same statement tells a stale version from a missing row (ADR 0006 explains why it is one statement). An empty patch issues no `UPDATE`: it returns the current row, unbumped, when it is at the expected version, and `VersionConflict` otherwise, so a stale caller learns of the conflict either way. On a table that also has a soft-delete column, a deleted row is `NotFound`, patched or not, and stays unchanged. On a table with a soft-delete column too, a deleted row is `NotFound`, never a conflict. `UpdateOutcome` is defined once, in `db/mod.rs`, and only when some table has a versioned `update`.
**The tenant column** scopes a table to one tenant. Every generated function on a table that has it takes the tenant id (the column's type, or its typed id) right after the executor, and every statement adds `<col> = <tenant>`: reads, unique-key lookups, `search` and `search_after` (after the filter's conditions), `create` (which inserts the parameter), `update` (in the shared key condition, so the versioned and empty-patch paths are scoped too), and `delete`, `restore`, and `delete_permanently`. A row of another tenant is therefore missing to every function, with or without row-level security. The column is left out of create inputs and update patches, so a row's tenant comes only from the scope and never changes. When the column is part of the primary key, the tenant parameter replaces that key column: the key (typed id, key struct, `search_after` cursor) is the remaining columns, and a unique key loses the tenant column from its columns and its lookup's name. The search filter has no field for the column (not even `<col>_is_null`) and the sort key no variant for it: every row `search` returns has the same tenant, so a condition on it could only repeat the scope or match nothing. Relationship filters are unchanged.

**The session helpers** (`db/session.rs`, `session` being a reserved module name) exist when `tenant_setting` or `user_setting` is set: `set_tenant(&mut tx, id)` and `set_user(&mut tx, id)` run `SELECT set_config(<setting>, **The soft-delete column** must be, true)` with `id` rendered by `Display`, so the value lasts until the transaction ends. They take `&mut PgConnection` rather than a generic executor because the setting only matters on the connection the following statements use. Row-level security is the database's second line of defense; the generated tenant parameter does not depend on it.

**The soft-delete column** must be a nullable `TIMESTAMP` or `TIMESTAMPTZ`; any other type fails generation, naming the table and column. On a table that has it, `delete` becomes `UPDATE … SET <col> = now() WHERE <key> AND <col> IS NULL`, `restore` clears it, and `delete_permanently` is the real `DELETE`. All three report whether a row changed. Every generated read adds `<col> IS NULL`, except `search` and `search_after` when the filter's `include_deleted` is set. `update` adds it too, so a deleted row counts as missing. The column is left out of inputs and patches, so only `delete` and `restore` change it. A partial unique index whose condition is exactly `<col> IS NULL` becomes a unique key with a `get_by_<cols>` lookup, which finds the live row. Such an index is still **not** accepted as an `ON CONFLICT (cols)` target in query files. PostgreSQL infers a partial index only from a target that repeats its `WHERE` condition, and the query parser cannot read that form.

## How the generator sees conventions

The configuration is applied **once, to the schema model**, by `generator::conventions::apply`, right after typed ids are resolved and before any renderer runs. It records what each column or table *is*; renderers ask the model, never the configuration:

- `Column::is_managed` (this ADR) and `Table::version_column`. `Column::is_insertable`/`is_updatable` consult it, so models and writes needed no change.
- Later conventions follow the same shape: a single-column convention is recorded where the renderers need it (for example, the table's soft-delete, tenant, or version column, set only when the table has that column), so a renderer's question is "does this table have a tenant column?", not "is a tenant convention configured, and does this table match it?".
- Settings that are not about a column (`tenant_setting`, `user_setting`) are read from the `&Conventions` that `plan_files_with_queries` already receives, for the generated session helpers.

Only the tables generated in a run are annotated; the parser and the query analyzer see the schema as written.

## Considered Options

- **Pass `&Conventions` to every renderer.** Rejected: every renderer would repeat the "does this column match?" check, and the per-column rules (`is_insertable`, `is_updatable`) already live on the model. Annotating once keeps the renderers' signatures stable as conventions land.
- **Annotate in the parser.** Rejected: conventions are generation policy, not schema facts. The parser and query analysis stay convention-free, which is what keeps query files untouched.
- **Turn conventions on by default, or infer them from column names.** Rejected: a schema that merely has a `deleted_at` column would silently change behavior. Opt-in with a one-line preset costs the Rok ERP nothing.
- **Merge whole `[conventions]` sections instead of keys.** Rejected: a payload adding one key would silently drop the file's others, unlike every other key's precedence.
- **CLI flags for conventions.** Deferred: conventions are project policy that belongs in a file, and the JSON payload covers scripted use.
