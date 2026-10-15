# Write helpers: `create`, `update`, and `delete`

Phase 5 adds writes to each resource's `write.rs`, driven by two column flags the parser now records: `has_default` (the database can supply a value: `DEFAULT`, serial, identity, or `GENERATED`) and `is_generated` (the database always owns the value: serial, identity, or `GENERATED`).

`create(pool, input: NewX) -> Result<X, sqlx::Error>` inserts one row and returns it via `RETURNING`. `NewX` omits every generated column and turns every defaulted column into an `Option<T>`; when such an option is `None` the column is left out of the `INSERT` entirely so the database default applies. Columns without a default are required fields. The column list and placeholders are built at runtime, and when every column is optional an empty set falls back to `INSERT ... DEFAULT VALUES`.

`update(pool, keys..., input: UpdateX) -> Result<Option<X>, sqlx::Error>` is a partial patch. `UpdateX` carries every non-key, non-generated column as a patch type: `Option<T>` for ordinary columns and `Option<Option<T>>` for nullable ones, so "leave unchanged" (`None`) is distinct from "set `NULL`" (`Some(None)`). Only the present fields are written, the affected row is returned via `RETURNING`, and a call with no fields set delegates to `get_by_id` rather than issuing an empty `UPDATE`.

`delete(pool, keys...) -> Result<bool, sqlx::Error>` runs `DELETE` and returns whether a row was removed (`rows_affected() > 0`). A table with no primary key gets `create` only; `update` and `delete` are omitted with a comment, matching the read side's treatment of `get_by_id`.

## Considered Options

- **`insert`/`update`/`delete` naming** — rejected: `insert` collides with the Rust term for container operations, and `create` matches the surrounding "resource" vocabulary.
- **Full-row `UpdateX` (every column required)** — rejected: forces read-modify-write and races under concurrency; a patch expresses intent directly.
- **`Option<T>` for nullable update fields** — rejected: cannot express "set NULL", which is a common and legitimate update.
- **Returning `()` from `delete`** — rejected: the caller usually needs to know whether anything matched.
- **Always emitting `create`** — rejected for tables whose every column is generated: there is nothing for the caller to supply.
