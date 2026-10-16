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

**The version column** is always managed, whether or not `managed_columns` lists it: the database advances it (a `touch_row`-style trigger), and a caller who could write it could defeat the check. On a table that has it, `update` takes `expected_version` and returns `UpdateOutcome<Row>`: `Updated(row)`, `NotFound`, or `VersionConflict`. The version check is part of the `UPDATE`'s condition, and the same statement tells a stale version from a missing row (ADR 0006 explains why it is one statement). An empty patch issues no `UPDATE`: it returns the current row, unbumped, when it is at the expected version, and `VersionConflict` otherwise, so a stale caller learns of the conflict either way. `UpdateOutcome` is defined once, in `db/mod.rs`, and only when some table has a versioned `update`.

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
