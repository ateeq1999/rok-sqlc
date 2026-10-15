#![deny(warnings)]

pub mod db;

// Compile-only smoke check for the generated executor-generic API. The
// fixture_compile test runs `cargo check`, so this proves callers can pass
// either a pool or a caller-owned transaction to the generated functions.
#[allow(dead_code)]
async fn executor_smoke(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    let _ = db::suppliers::read::list(pool).await?;

    let mut tx = pool.begin().await?;
    let _ = db::suppliers::read::list(&mut *tx).await?;
    db::suppliers::write::delete(&mut *tx, uuid::Uuid::nil()).await?;
    tx.commit().await?;

    Ok(())
}
