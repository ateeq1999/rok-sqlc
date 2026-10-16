# CRUD functions are generic over `PgExecutor`

Every generated function takes its executor generically — `pub async fn get_by_id<'e, E>(executor: E, ...) where E: sqlx::PgExecutor<'e>` — instead of a concrete `&PgPool`. Callers pass `&pool` or `&mut *tx` to the same function, so the transaction boundary is theirs to own and no separate `_tx` variants exist.

The schema cannot derive business transaction functions like "mark user active"; those belong to the query-file phase. What the schema *can* do is make its own CRUD composable inside a transaction, which is exactly what a generic executor buys. The parameter is named `executor` rather than `pool` because it is no longer necessarily a pool.

`sqlx::PgExecutor` is a blanket impl over every `Executor<Database = Postgres>`, so it covers `&PgPool` and `&mut PgConnection`. `Transaction` itself is not an executor in SQLx 0.9, as in 0.8 (that impl is not provided), so callers deref it with `&mut *tx`. With that, both of these compile against the same generated code:

```rust
db::users::read::get_by_id(&pool, id).await?;

let mut tx = pool.begin().await?;
db::users::write::delete(&mut *tx, id).await?;
tx.commit().await?;
```

The `fixture-app` canary carries a compile-only function that calls a generated read with a pool and a generated delete with a transaction, so any signature regression fails `cargo check`.

## Functions that need more than one statement

An `E: PgExecutor<'e>` executor is consumed by its first query, so a generated function can run exactly one statement through it. Rather than change the bound for functions that need a decision based on a first result, such a function does its work in **one statement**. The first is the versioned `update` (ADR 0010): a CTE finds the row by key alone (`rok_sqlc_target`), a second CTE updates it only at the expected version (`rok_sqlc_updated`), and the outer `SELECT` returns no row (not found), a row flagged as not applied (version conflict), or the updated row. Both CTEs see the same snapshot, so there is no gap between an update and a check for another write to slip into. (One edge remains: a row that a concurrent transaction deletes while the `UPDATE` waits on its lock is reported as a version conflict, because the snapshot still sees it; a retry reports `NotFound`.) Callers keep passing `&pool` or `&mut *tx` unchanged.

Considered for that case:

- **A follow-up existence query after an `UPDATE` that matched nothing**, with the bound changed to `sqlx::Acquire` (`&PgPool` and `&mut PgConnection` both implement it). Rejected: one function would have a different bound from all the others, and the two statements are not atomic outside a transaction, so a row deleted between them could be misreported.
- **Requiring a `Copy` executor such as `&PgPool`**. Rejected: it shuts out `&mut *tx`, which this ADR exists to support.

## Considered Options

- **Parallel `*_tx` variants taking `&mut Transaction`** — rejected: doubles the generated surface and forces callers to pick a variant, when one generic signature covers both.
- **Keep `&PgPool` and add a `tx.rs` of hand-written transaction functions** — rejected: those are application-specific statements (Phase 7 query files), and shipping examples only for their own sake adds surface with no schema-derived value.
- **Concrete `&mut PgConnection` executor** — rejected: it cannot accept a `&PgPool`, so the pool path would still need its own functions.
