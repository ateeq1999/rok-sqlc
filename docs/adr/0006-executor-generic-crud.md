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

## Considered Options

- **Parallel `*_tx` variants taking `&mut Transaction`** — rejected: doubles the generated surface and forces callers to pick a variant, when one generic signature covers both.
- **Keep `&PgPool` and add a `tx.rs` of hand-written transaction functions** — rejected: those are application-specific statements (Phase 7 query files), and shipping examples only for their own sake adds surface with no schema-derived value.
- **Concrete `&mut PgConnection` executor** — rejected: it cannot accept a `&PgPool`, so the pool path would still need its own functions.
