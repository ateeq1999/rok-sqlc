//! Must NOT compile: tests/fixture_compile/ enables the `typed-id-mixup`
//! feature and expects `cargo check` to reject this file, proving that one
//! table's typed id cannot stand in for another's.

use crate::db;

#[allow(dead_code)]
async fn category_id_is_not_a_product_id(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    let category_id = db::categories::models::CategoryId::from(1);
    let _ = db::products::read::get_by_id(pool, category_id).await?;
    Ok(())
}
