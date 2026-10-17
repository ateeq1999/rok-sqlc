//! Annotated query files.

use super::*;

pub(super) async fn annotated_queries(pool: &PgPool) {
    use db::products::models::ProductId;
    use db::queries::inventory::{self as queries, CreateProductsParams};

    let category = queries::create_category(pool, "query-cat", None)
        .await
        .unwrap();
    assert_eq!(category.name, "query-cat");
    assert_eq!(category.description, None);

    let supplier = db::suppliers::write::create(
        pool,
        db::suppliers::models::NewSupplier {
            id: uuid::Uuid::from_u128(900).into(),
            name: "Acme".to_owned(),
            contact_email: None,
            is_active: None,
            created_at: None,
        },
    )
    .await
    .unwrap();

    let inserted = queries::create_products(
        pool,
        CreateProductsParams {
            id: ProductId::from(uuid::Uuid::from_u128(101)),
            sku: "Q-1".to_owned(),
            name: "Query hammer".to_owned(),
            category_id: category.id,
            supplier_id: Some(supplier.id),
            id_2: ProductId::from(uuid::Uuid::from_u128(102)),
            sku_2: "Q-2".to_owned(),
            name_2: "Query anvil".to_owned(),
            category_id_2: category.id,
            supplier_id_2: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        inserted, 2,
        "both VALUES rows bind to the right placeholders"
    );

    let hammer = queries::get_product_by_sku(pool, "Q-1")
        .await
        .unwrap()
        .expect("Q-1 exists");
    assert_eq!(hammer.id, ProductId::from(uuid::Uuid::from_u128(101)));
    assert_eq!(hammer.name, "Query hammer");
    assert_eq!(
        queries::get_product_by_sku(pool, "missing").await.unwrap(),
        None
    );

    queries::update_product_stock(pool, ProductId::from(uuid::Uuid::from_u128(101)), 50)
        .await
        .unwrap();
    let low_stock: Vec<String> = queries::list_products_by_category(pool, category.id, 10)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.sku)
        .collect();
    assert_eq!(low_stock, ["Q-2"], "only the anvil has stock <= 10");

    let joined = queries::list_products_with_category(pool, &["Q-1".to_owned(), "Q-2".to_owned()])
        .await
        .unwrap();
    assert_eq!(joined.len(), 2);
    assert_eq!(joined[0].category_name, "query-cat");
    assert_eq!(joined[0].supplier_name.as_deref(), Some("Acme"));
    assert_eq!(
        joined[1].supplier_name, None,
        "LEFT JOIN without a supplier"
    );

    assert_eq!(
        queries::count_products_in_category(pool, category.id)
            .await
            .unwrap(),
        2
    );
    let stock = queries::category_stock(pool).await.unwrap();
    let ours = stock
        .iter()
        .find(|row| row.category_id == category.id)
        .expect("category has stock");
    assert_eq!(ours.total_stock, 50);

    let stocked = queries::stocked_categories(pool, 1, category.id)
        .await
        .unwrap();
    assert_eq!(stocked.len(), 1, "the CTE pre-aggregates per category");
    assert_eq!(stocked[0].id, category.id);
    assert_eq!(stocked[0].category_name, "query-cat");
    assert_eq!(
        stocked[0].product_count, 1,
        "only the hammer has stock >= 1"
    );
    assert_eq!(stocked[0].total_stock, Some(50));
    assert!(
        queries::stocked_categories(pool, 51, category.id)
            .await
            .unwrap()
            .is_empty(),
        "the CTE placeholder filters before aggregation"
    );

    let found = queries::search_products(pool, "%ANVIL%").await.unwrap();
    assert_eq!(found, ["Q-2"]);

    assert_eq!(
        queries::delete_product_by_sku(pool, "Q-2").await.unwrap(),
        1
    );
    assert_eq!(
        queries::delete_product_by_sku(pool, "Q-2").await.unwrap(),
        0
    );
}

/// Each query file is its own module, so both define `search_products`.
/// Runs after `annotated_queries`, which leaves product Q-1 supplied by Acme.
pub(super) async fn query_modules_per_file(pool: &PgPool) {
    use db::queries::{inventory, suppliers};

    let globex_id = db::suppliers::models::SupplierId::from(uuid::Uuid::from_u128(901));
    let globex = suppliers::create_supplier(pool, globex_id, "Globex")
        .await
        .unwrap();
    assert_eq!(globex.name, "Globex");
    assert!(globex.is_active, "the column default applies");

    assert_eq!(
        suppliers::search_products(pool, "Acme").await.unwrap(),
        ["Q-1"]
    );
    assert!(
        suppliers::search_products(pool, "Globex")
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        inventory::search_products(pool, "%hammer%").await.unwrap(),
        ["Q-1"]
    );
}
