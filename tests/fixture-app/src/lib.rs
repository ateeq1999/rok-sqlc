//! The code rok-sqlc generates from its fixture schema and query files,
//! compiled the way a consuming crate compiles it.
//!
//! `src/db/`, `src/plain_db/` (generated without serde, see
//! `plain_schema.sql`) and `src/validated_db/` (generated with validation
//! rules, see `validated_schema.sql`) are written by
//! `tests/fixture_compile.rs` in rok-sqlc and are not checked in. The lints
//! below are the generated-code quality bar (ADR 0013).

#![deny(missing_docs, warnings)]
// The selected pedantic lints. They warn, so `deny(warnings)` and
// `cargo clippy -- -D warnings` turn any hit into an error.
#![warn(
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::explicit_iter_loop,
    clippy::implicit_clone,
    clippy::items_after_statements,
    clippy::manual_let_else,
    clippy::match_same_arms,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::redundant_closure_for_method_calls,
    clippy::ref_option,
    clippy::return_self_not_must_use,
    clippy::semicolon_if_nothing_returned,
    clippy::single_match_else,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::unnecessary_wraps
)]

pub mod db;
pub mod plain_db;
pub mod validated_db;

#[cfg(feature = "typed-id-mixup")]
mod typed_id_mixup;

// Compile-only smoke check for the generated executor-generic API. The
// fixture_compile test runs `cargo clippy`, so this proves callers can pass
// either a pool or a caller-owned transaction to the generated functions.
#[allow(dead_code)]
async fn executor_smoke(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    let _ = db::suppliers::read::list(pool).await?;

    let mut tx = pool.begin().await?;
    let _ = db::suppliers::read::list(&mut *tx).await?;
    db::suppliers::write::delete(&mut *tx, uuid::Uuid::nil().into()).await?;
    tx.commit().await?;

    Ok(())
}
