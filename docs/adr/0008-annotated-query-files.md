# Annotated query files: infer from context, never guess

`rok-sqlc generate --queries queries.sql` turns SQLC-style annotated queries into `db/queries.rs`, one typed function per query. This is the second lane of the two-lane principle: the schema generates baseline CRUD, and query files carry application-specific SQL.

```sql
-- name: ListProductsByCategory :many
SELECT id, sku, name, price_cents FROM products WHERE category_id = $1 ORDER BY name, id;
```

**Return modes.** `:one` → `fetch_one` (no row is `RowNotFound`), `:maybe_one` → `fetch_optional`, `:many` → `fetch_all`, `:exec` → `execute` returning `()`, and `:execrows` → `execute` returning `rows_affected()`. sqlc's `:one` is ambiguous about "no row", so the two outcomes get two names.

**Parameter types come only from contexts that determine them.** These are a comparison, `IN`, `BETWEEN`, or `LIKE` against a known column; `= ANY($n)` (which becomes `Vec<T>`); an `INSERT … VALUES` position or `UPDATE … SET` target (nullable columns become `Option<T>`); `ON CONFLICT … DO UPDATE SET`; `LIMIT`/`OFFSET` (`i64`); and explicit casts. A placeholder in any other position (for example `lower($1)`) is an error that names the fix: `-- param: $1 <name> <RustType>`. Gaps (`$2` without `$1`) and one placeholder used as two types are errors too. Parameter names come from the column, de-duplicated with `_2`, `_3`.

**Result columns are resolved against the schema.** `*`, `t.*`, plain and qualified column references, aliases, and `COUNT(…)` (`i64`) are inferred; a `LEFT`/`RIGHT`/`FULL` join makes the outer side's columns `Option`. Any other expression needs an alias and a `-- column: name <RustType>` override. A result that is exactly a generated table's columns in order reuses that resource's row struct (`RETURNING *` → `super::products::models::Product`); a single column decodes with `query_scalar`; otherwise a `{Name}Row` struct is generated. More than three parameters become a `{Name}Params` struct.

**The SQL is embedded verbatim** and bound positionally, so `$n` always receives the n-th parameter. Queries are not compile-time checked; no database is needed to generate.

**Strictness follows ADR 0001.** A malformed annotation, an unknown table or column, a reference to a table the schema parser rejected, an untypable placeholder, or a multi-statement query (`BEGIN; …; COMMIT;`) fails the run and writes nothing; `--lenient` skips such queries with warnings. Transactions remain the caller's: run several generated functions on `&mut *tx` (ADR 0006).

## Considered Options

- **Ask PostgreSQL for types (`PREPARE`/`sqlx::query!`)**: rejected as the default, because generation must work offline (README §15). It remains possible as a future opt-in mode.
- **Default untypable parameters to `String`**: rejected, because a wrong type compiles and then fails at runtime, which is exactly the guess this tool refuses to make.
- **One output module per query file**: rejected for now; a single `--queries` file maps to `db/queries.rs`, which keeps the root `mod.rs` merge rules simple.
- **Validating `ON CONFLICT` targets against unique constraints**: deferred; the schema model does not record unique constraints yet, so `ON CONFLICT (sku)` without a unique index fails only at runtime.
