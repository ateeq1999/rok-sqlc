//! Executes the generated CRUD code against a real PostgreSQL database.
//!
//! Runs only when `ROK_SQLC_TEST_DATABASE_URL` is set. The database is reset
//! (its `public`, `app`, and `legacy` schemas are dropped), so point it at a throwaway
//! database, never one holding data you care about.

use rok_sqlc_fixture_app::db;
use rust_decimal::Decimal;
use sqlx::PgPool;

use db::categories::models::CategoryId;
use db::patch_targets::models::{NewPatchTarget, PatchTarget, PatchTargetId, UpdatePatchTarget};

async fn fresh_database() -> Option<PgPool> {
    let Ok(url) = std::env::var("ROK_SQLC_TEST_DATABASE_URL") else {
        eprintln!("skipping: ROK_SQLC_TEST_DATABASE_URL is not set");
        return None;
    };
    let pool = PgPool::connect(&url)
        .await
        .expect("connect to test database");
    sqlx::raw_sql(
        "DROP SCHEMA IF EXISTS app CASCADE; DROP SCHEMA IF EXISTS legacy CASCADE;
         DROP SCHEMA IF EXISTS public CASCADE; CREATE SCHEMA public;",
    )
    .execute(&pool)
    .await
    .expect("reset test database");
    // A table outside the generator's input that shares its name with the
    // generated `app.patch_targets`; `app.patch_links` references both.
    sqlx::raw_sql(
        "CREATE SCHEMA legacy; CREATE TABLE legacy.patch_targets (id BIGINT PRIMARY KEY);",
    )
    .execute(&pool)
    .await
    .expect("create the ungenerated legacy table");
    sqlx::raw_sql(db::schema::SCHEMA_SQL)
        .execute(&pool)
        .await
        .expect("apply fixture schema");
    Some(pool)
}

/// One scenario per test would race on the shared database reset, so the
/// scenarios run in sequence against one fresh database.
#[tokio::test]
async fn generated_crud_against_postgres() {
    let Some(pool) = fresh_database().await else {
        return;
    };
    insert_applies_defaults_and_explicit_nulls(&pool).await;
    every_partial_update_combination_binds_correctly(&pool).await;
    empty_and_missing_updates(&pool).await;
    delete_reports_existing_and_missing_rows(&pool).await;
    transactions_stay_with_the_caller(&pool).await;
    all_defaulted_insert_uses_default_values(&pool).await;
    reserved_and_quoted_identifiers_round_trip(&pool).await;
    table_without_primary_key(&pool).await;
    uuid_keys_listing_and_pagination(&pool).await;
    composite_keys_arrays_and_numeric(&pool).await;
    annotated_queries(&pool).await;
    query_modules_per_file(&pool).await;
    search_filters_and_pages(&pool).await;
    altered_table_has_its_final_columns(&pool).await;
    enums_keep_their_exact_labels(&pool).await;
    value_list_columns_round_trip(&pool).await;
    typed_ids_flow_from_rows_to_foreign_keys(&pool).await;
    foreign_keys_resolve_to_the_table_in_their_schema(&pool).await;
    unique_key_lookups_find_one_row_or_none(&pool).await;
    managed_columns_are_left_to_the_database(&pool).await;
    versioned_updates_detect_concurrent_edits(&pool).await;
    text_keys_are_borrowed(&pool).await;
    rows_round_trip_through_json(&pool).await;
    soft_deleted_rows_are_hidden_restored_and_removed(&pool).await;
    tenant_rows_are_invisible_to_other_tenants(&pool).await;
    a_tenant_in_the_primary_key_is_passed_once(&pool).await;
    row_level_security_sees_the_session_tenant(&pool).await;
    belongs_to_loader_fetches_every_parent_in_one_query(&pool).await;
    has_many_loader_stays_in_the_tenant_and_skips_deleted_rows(&pool).await;
    several_foreign_keys_to_one_table_get_distinct_names(&pool).await;
    unique_foreign_key_loads_one_row(&pool).await;
    self_reference_loads_parent_and_children(&pool).await;
    belongs_to_filter_matches_each_row_once(&pool).await;
    has_many_filter_matches_each_parent_once(&pool).await;
    relationship_filters_skip_deleted_and_other_tenant_rows(&pool).await;
    self_reference_filters_and_nesting_depth(&pool).await;
    renamed_relationships_load_under_their_new_names(&pool).await;
    joined_read_fetches_a_row_with_its_parent_in_one_query(&pool).await;
    joined_read_of_a_nullable_foreign_key_without_a_parent(&pool).await;
    joined_read_respects_soft_delete_and_tenant_on_both_sides(&pool).await;
    renamed_relationships_filter_under_their_new_names(&pool).await;
    many_to_many_attach_detach_and_sync(&pool).await;
    soft_delete_join_rows_are_restored_not_duplicated(&pool).await;
    plain_join_table_deletes_detached_pairs(&pool).await;
    many_to_many_filters_go_through_the_join_table(&pool).await;
}

/// A category's typed id goes straight into a product's foreign key field
/// and into a product filter; typed ids hash, order, display, and convert.
async fn typed_ids_flow_from_rows_to_foreign_keys(pool: &PgPool) {
    use std::collections::HashSet;

    use db::categories::models::{CategoryId, NewCategory};
    use db::products::filter::ProductFilter;
    use db::products::models::{NewProduct, ProductId};
    use db::search::{Ordered, Page, Sort};

    let category = db::categories::write::create(
        pool,
        NewCategory {
            name: "typed".to_owned(),
            description: None,
        },
    )
    .await
    .unwrap();
    let product_id = ProductId::from(uuid::Uuid::from_u128(7_001));
    let product = db::products::write::create(
        pool,
        NewProduct {
            id: product_id,
            sku: "T-1".to_owned(),
            name: "Typed".to_owned(),
            category_id: category.id,
            supplier_id: None,
            price_cents: None,
            stock_on_hand: None,
            reorder_level: None,
            weight_kg: None,
            discontinued: None,
            released_on: None,
            created_at: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(product.category_id, category.id);

    let found = db::products::read::search(
        pool,
        ProductFilter {
            category_id: vec![Ordered::Eq(category.id)],
            ..Default::default()
        },
        Sort::asc(db::products::filter::ProductSortKey::Sku),
        Page::first(10),
    )
    .await
    .unwrap();
    assert_eq!(
        found,
        vec![product.clone()],
        "a category's id filters its products"
    );

    let fetched = db::products::read::get_by_id(pool, product_id)
        .await
        .unwrap();
    assert_eq!(fetched, Some(product));
    let parent = db::categories::read::get_by_id(pool, category.id)
        .await
        .unwrap();
    assert_eq!(parent.map(|row| row.name), Some("typed".to_owned()));

    // `Copy`: the id is used again after being passed by value above.
    let raw: i64 = category.id.into();
    assert_eq!(CategoryId::from(raw), category.id);
    assert_eq!(category.id.to_string(), raw.to_string());
    assert_eq!(
        product_id.to_string(),
        "00000000-0000-0000-0000-000000001b59"
    );
    assert!(CategoryId::from(1) < CategoryId::from(2));
    let distinct: HashSet<CategoryId> = [category.id, category.id, CategoryId::from(raw + 1)]
        .into_iter()
        .collect();
    assert_eq!(distinct.len(), 2);
}

/// `app.patch_links` references `app.patch_targets` (generated, so the field
/// is a `PatchTargetId`) and `legacy.patch_targets` (not generated, so the
/// field keeps `i64`).
async fn foreign_keys_resolve_to_the_table_in_their_schema(pool: &PgPool) {
    use db::patch_links::models::NewPatchLink;
    use db::patch_targets::models::PatchTargetId;

    let target = db::patch_targets::write::create(pool, base_patch_target())
        .await
        .unwrap();
    sqlx::raw_sql("INSERT INTO legacy.patch_targets (id) VALUES (42)")
        .execute(pool)
        .await
        .unwrap();
    let target_id: PatchTargetId = target.id;
    let legacy_target_id: Option<i64> = Some(42);
    let link = db::patch_links::write::create(
        pool,
        NewPatchLink {
            target_id,
            legacy_target_id,
        },
    )
    .await
    .unwrap();
    assert_eq!(link.target_id, target.id);
    assert_eq!(link.legacy_target_id, Some(42));
}

/// Seeds five products in a fresh category, priced 100..=500, and returns
/// the category id.
async fn seed_search_products(pool: &PgPool) -> CategoryId {
    use db::categories::models::NewCategory;
    use db::products::models::NewProduct;

    let category = db::categories::write::create(
        pool,
        NewCategory {
            name: "search".to_owned(),
            description: None,
        },
    )
    .await
    .unwrap();
    for (index, name) in ["Bolt", "Anchor", "Clamp 50%", "Drill_bit", "Easel"]
        .into_iter()
        .enumerate()
    {
        let n = index as i64 + 1;
        db::products::write::create(
            pool,
            NewProduct {
                // Keys descend as rows are inserted, so physical order differs from key order.
                id: uuid::Uuid::from_u128(506 - n as u128).into(),
                sku: format!("S-{n}"),
                name: name.to_owned(),
                category_id: category.id,
                supplier_id: None,
                price_cents: Some(n * 100),
                stock_on_hand: None,
                reorder_level: None,
                weight_kg: (n % 2 == 1).then_some(n as f64),
                discontinued: Some(n == 5),
                released_on: chrono::NaiveDate::from_ymd_opt(2024, 1, n as u32),
                created_at: None,
            },
        )
        .await
        .unwrap();
    }
    category.id
}

fn skus(rows: &[db::products::models::Product]) -> Vec<String> {
    rows.iter().map(|row| row.sku.clone()).collect()
}

async fn search_filters_and_pages(pool: &PgPool) {
    use db::products::filter::{ProductFilter, ProductSortKey};
    use db::products::read::search;
    use db::search::{Ordered, Page, Sort};

    let category = seed_search_products(pool).await;
    let in_category = || ProductFilter {
        category_id: vec![Ordered::Eq(category)],
        ..Default::default()
    };

    let ranged = search(
        pool,
        ProductFilter {
            price_cents: vec![Ordered::Ge(200), Ordered::Lt(500)],
            ..in_category()
        },
        Sort::asc(ProductSortKey::PriceCents),
        Page::first(10),
    )
    .await
    .unwrap();
    assert_eq!(skus(&ranged), ["S-2", "S-3", "S-4"], "conditions are ANDed");

    let between = search(
        pool,
        ProductFilter {
            price_cents: vec![Ordered::Between(150, 350)],
            ..in_category()
        },
        Sort::asc(ProductSortKey::PriceCents),
        Page::first(10),
    )
    .await
    .unwrap();
    assert_eq!(skus(&between), ["S-2", "S-3"]);

    let second_page = search(
        pool,
        in_category(),
        Sort::asc(ProductSortKey::PriceCents),
        Page::first(2).next(),
    )
    .await
    .unwrap();
    assert_eq!(skus(&second_page), ["S-3", "S-4"]);

    text_filters_escape_like_wildcards(pool, category).await;
    null_checks_and_equality_filters(pool, category).await;
    sorting_is_allowlisted_and_stable(pool, category).await;
    keyset_pagination_follows_the_primary_key(pool, category).await;
}

async fn keyset_pagination_follows_the_primary_key(pool: &PgPool, category: CategoryId) {
    use db::feature_line_items::filter::FeatureLineItemFilter;
    use db::feature_line_items::models::{FeatureLineItemKey, NewFeatureLineItem};
    use db::products::filter::ProductFilter;
    use db::products::read::search_after;
    use db::search::{Limit, MAX_PAGE_SIZE, Ordered, Page};

    let in_category = || ProductFilter {
        category_id: vec![Ordered::Eq(category)],
        ..Default::default()
    };
    let mut after = None;
    let mut pages = Vec::new();
    loop {
        let page = search_after(pool, in_category(), after, Limit::new(2))
            .await
            .unwrap();
        let Some(last) = page.last() else {
            break;
        };
        after = Some(last.id);
        pages.push(skus(&page));
    }
    assert_eq!(
        pages,
        [vec!["S-5", "S-4"], vec!["S-3", "S-2"], vec!["S-1"]],
        "keys 501..=505 belong to S-5..=S-1"
    );

    let id = uuid::Uuid::from_u128;
    for (order_id, product_id) in [(1, 2), (1, 1), (2, 1)] {
        db::feature_line_items::write::create(
            pool,
            NewFeatureLineItem {
                order_id: id(order_id),
                product_id: id(product_id),
                labels: Vec::new(),
                weekdays: None,
                reporter_ids: None,
                cut_off: None,
                unit_price: Decimal::ONE,
            },
        )
        .await
        .unwrap();
    }
    let after_first = db::feature_line_items::read::search_after(
        pool,
        FeatureLineItemFilter::default(),
        Some(FeatureLineItemKey {
            order_id: id(1),
            product_id: id(1),
        }),
        Limit::new(10),
    )
    .await
    .unwrap();
    let keys: Vec<(u128, u128)> = after_first
        .iter()
        .map(|row| (row.order_id.as_u128(), row.product_id.as_u128()))
        .collect();
    assert_eq!(keys, [(1, 2), (2, 1)], "composite keys compare as a row");

    let clamped = Page::new(10_000, -5);
    assert_eq!((clamped.limit(), clamped.offset()), (MAX_PAGE_SIZE, 0));
    let last = Page::new(MAX_PAGE_SIZE, i64::MAX - 10).next();
    assert_eq!(
        last.offset(),
        i64::MAX,
        "next() saturates instead of overflowing"
    );
    let one = search_after(pool, in_category(), None, Limit::new(0))
        .await
        .unwrap();
    assert_eq!(one.len(), 1, "a limit below 1 is clamped to 1");
    assert_eq!(Limit::new(10_000).get(), MAX_PAGE_SIZE);
}

async fn sorting_is_allowlisted_and_stable(pool: &PgPool, category: CategoryId) {
    use db::products::filter::{ProductFilter, ProductSortKey};
    use db::products::read::search;
    use db::search::{Ordered, Page, Sort};

    let sorted = |sort: Sort<ProductSortKey>| async move {
        let rows = search(
            pool,
            ProductFilter {
                category_id: vec![Ordered::Eq(category)],
                ..Default::default()
            },
            sort,
            Page::first(10),
        )
        .await
        .unwrap();
        skus(&rows)
    };

    assert_eq!(
        sorted(Sort::desc(ProductSortKey::PriceCents)).await,
        ["S-5", "S-4", "S-3", "S-2", "S-1"]
    );
    assert_eq!(
        sorted(Sort::asc(ProductSortKey::Name)).await,
        ["S-2", "S-1", "S-3", "S-4", "S-5"],
        "Anchor, Bolt, Clamp, Drill, Easel"
    );
    // Four products tie on `discontinued = false`; the primary key orders them.
    assert_eq!(
        sorted(Sort::asc(ProductSortKey::Discontinued)).await,
        ["S-4", "S-3", "S-2", "S-1", "S-5"]
    );
    assert_eq!(
        sorted(Sort::desc(ProductSortKey::Discontinued)).await,
        ["S-5", "S-4", "S-3", "S-2", "S-1"]
    );
}

async fn null_checks_and_equality_filters(pool: &PgPool, category: CategoryId) {
    use db::products::filter::{ProductFilter, ProductSortKey};
    use db::products::read::search;
    use db::search::{Equality, Ordered, Page, Sort};

    let skus_where = |filter: ProductFilter| async move {
        let rows = search(
            pool,
            ProductFilter {
                category_id: vec![Ordered::Eq(category)],
                ..filter
            },
            Sort::asc(ProductSortKey::PriceCents),
            Page::first(10),
        )
        .await
        .unwrap();
        skus(&rows)
    };

    // Odd-numbered products have a weight; even-numbered ones are NULL.
    let unweighed = skus_where(ProductFilter {
        weight_kg_is_null: Some(true),
        ..Default::default()
    })
    .await;
    assert_eq!(unweighed, ["S-2", "S-4"]);
    let weighed = skus_where(ProductFilter {
        weight_kg_is_null: Some(false),
        weight_kg: vec![Ordered::Gt(1.0)],
        ..Default::default()
    })
    .await;
    assert_eq!(weighed, ["S-3", "S-5"]);

    let discontinued = skus_where(ProductFilter {
        discontinued: vec![Equality::Eq(true)],
        ..Default::default()
    })
    .await;
    assert_eq!(discontinued, ["S-5"]);

    let by_id = skus_where(ProductFilter {
        id: vec![Equality::NotIn(vec![
            uuid::Uuid::from_u128(501).into(),
            uuid::Uuid::from_u128(503).into(),
        ])],
        supplier_id_is_null: Some(true),
        ..Default::default()
    })
    .await;
    assert_eq!(
        by_id,
        ["S-1", "S-2", "S-4"],
        "ids 501 and 503 are S-5 and S-3"
    );

    let picked = skus_where(ProductFilter {
        id: vec![Equality::In(vec![
            uuid::Uuid::from_u128(501).into(),
            uuid::Uuid::from_u128(502).into(),
        ])],
        discontinued: vec![Equality::Ne(true)],
        ..Default::default()
    })
    .await;
    assert_eq!(picked, ["S-4"], "502 is S-4; 501 (S-5) is discontinued");

    let numeric_sets = skus_where(ProductFilter {
        price_cents: vec![
            Ordered::In(vec![100, 200, 300, 400]),
            Ordered::NotIn(vec![200]),
            Ordered::Ne(400),
            Ordered::Le(300),
        ],
        ..Default::default()
    })
    .await;
    assert_eq!(numeric_sets, ["S-1", "S-3"]);

    // Product n was released on 2024-01-0n.
    let date = |day| chrono::NaiveDate::from_ymd_opt(2024, 1, day).unwrap();
    let released = skus_where(ProductFilter {
        released_on: vec![Ordered::Between(date(2), date(4))],
        ..Default::default()
    })
    .await;
    assert_eq!(released, ["S-2", "S-3", "S-4"]);

    let hour_ago = chrono::Utc::now() - chrono::Duration::hours(1);
    let recent = skus_where(ProductFilter {
        created_at: vec![Ordered::Gt(hour_ago)],
        ..Default::default()
    })
    .await;
    assert_eq!(recent.len(), 5, "timestamptz ranges bind as DateTime<Utc>");
    let old = skus_where(ProductFilter {
        created_at: vec![Ordered::Lt(hour_ago)],
        ..Default::default()
    })
    .await;
    assert!(old.is_empty());
}

async fn text_filters_escape_like_wildcards(pool: &PgPool, category: CategoryId) {
    use db::products::filter::{ProductFilter, ProductSortKey};
    use db::products::read::search;
    use db::search::{Ordered, Page, Sort, Text};

    let names = |filter: ProductFilter| async move {
        let rows = search(
            pool,
            ProductFilter {
                category_id: vec![Ordered::Eq(category)],
                ..filter
            },
            Sort::asc(ProductSortKey::PriceCents),
            Page::first(10),
        )
        .await
        .unwrap();
        rows.into_iter().map(|row| row.name).collect::<Vec<_>>()
    };

    let contains = names(ProductFilter {
        name: vec![Text::Contains("50%".to_owned())],
        ..Default::default()
    })
    .await;
    assert_eq!(contains, ["Clamp 50%"], "`%` is matched literally");

    let underscore = names(ProductFilter {
        name: vec![Text::Contains("_".to_owned())],
        ..Default::default()
    })
    .await;
    assert_eq!(underscore, ["Drill_bit"], "`_` is matched literally");

    let starts = names(ProductFilter {
        name: vec![Text::StartsWith("A".to_owned())],
        ..Default::default()
    })
    .await;
    assert_eq!(starts, ["Anchor"]);

    let ends = names(ProductFilter {
        name: vec![Text::EndsWith("el".to_owned())],
        ..Default::default()
    })
    .await;
    assert_eq!(ends, ["Easel"]);

    let exact = names(ProductFilter {
        sku: vec![Text::In(vec!["S-1".to_owned(), "S-4".to_owned()])],
        name: vec![Text::Ne("Bolt".to_owned())],
        ..Default::default()
    })
    .await;
    assert_eq!(exact, ["Drill_bit"]);

    let equal = names(ProductFilter {
        name: vec![Text::Eq("Easel".to_owned())],
        ..Default::default()
    })
    .await;
    assert_eq!(equal, ["Easel"]);

    let excluded = names(ProductFilter {
        sku: vec![Text::NotIn(vec!["S-1".to_owned(), "S-2".to_owned()])],
        ..Default::default()
    })
    .await;
    assert_eq!(excluded, ["Clamp 50%", "Drill_bit", "Easel"]);
}

async fn annotated_queries(pool: &PgPool) {
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
async fn query_modules_per_file(pool: &PgPool) {
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

fn base_patch_target() -> NewPatchTarget {
    NewPatchTarget {
        label: "base".to_owned(),
        note: Some("n0".to_owned()),
        score: Some(5),
        tag: Some(Some("t0".to_owned())),
    }
}

async fn insert_applies_defaults_and_explicit_nulls(pool: &PgPool) {
    use db::patch_targets::write::create;

    let defaults = create(
        pool,
        NewPatchTarget {
            label: "defaults".to_owned(),
            note: None,
            score: None,
            tag: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(defaults.score, 0, "omitted score uses DEFAULT 0");
    assert_eq!(
        defaults.tag.as_deref(),
        Some("none"),
        "omitted tag uses DEFAULT 'none'"
    );
    assert_eq!(defaults.note, None);
    assert_eq!(
        defaults.doubled,
        Some(0),
        "generated column is computed by the database"
    );

    let explicit = create(
        pool,
        NewPatchTarget {
            label: "explicit".to_owned(),
            note: Some("n".to_owned()),
            score: Some(4),
            tag: Some(None),
        },
    )
    .await
    .unwrap();
    assert_eq!(explicit.score, 4);
    assert_eq!(
        explicit.tag, None,
        "Some(None) inserts NULL instead of the default"
    );
    assert_eq!(explicit.note.as_deref(), Some("n"));
    assert_eq!(explicit.doubled, Some(8));
}

/// Applies every combination of unchanged / set / set-NULL over the four
/// updatable columns and checks the stored row field by field. A placeholder
/// or bind-order mistake surfaces as a value landing in the wrong column or
/// as a type error from PostgreSQL.
async fn every_partial_update_combination_binds_correctly(pool: &PgPool) {
    use db::patch_targets::{read, write};

    let labels = [None, Some("L".to_owned())];
    let notes = [None, Some(Some("N".to_owned())), Some(None)];
    let scores = [None, Some(7)];
    let tags = [None, Some(Some("T".to_owned())), Some(None)];

    let mut combinations = 0;
    for label in &labels {
        for note in &notes {
            for score in &scores {
                for tag in &tags {
                    let before = write::create(pool, base_patch_target()).await.unwrap();
                    let patch = UpdatePatchTarget {
                        label: label.clone(),
                        note: note.clone(),
                        score: *score,
                        tag: tag.clone(),
                    };

                    let mut expected = before.clone();
                    if let Some(value) = &patch.label {
                        expected.label = value.clone();
                    }
                    if let Some(value) = &patch.note {
                        expected.note = value.clone();
                    }
                    if let Some(value) = patch.score {
                        expected.score = value;
                        expected.doubled = Some(value * 2);
                    }
                    if let Some(value) = &patch.tag {
                        expected.tag = value.clone();
                    }

                    let updated = write::update(pool, before.id, patch.clone())
                        .await
                        .unwrap_or_else(|error| panic!("{patch:?} failed: {error}"))
                        .expect("row exists");
                    assert_eq!(updated, expected, "RETURNING row for {patch:?}");
                    let stored = read::get_by_id(pool, before.id).await.unwrap();
                    assert_eq!(stored, Some(expected), "stored row for {patch:?}");
                    combinations += 1;
                }
            }
        }
    }
    assert_eq!(combinations, 36);
}

async fn empty_and_missing_updates(pool: &PgPool) {
    use db::patch_targets::write;

    let row: PatchTarget = write::create(pool, base_patch_target()).await.unwrap();
    let unchanged = write::update(pool, row.id, UpdatePatchTarget::default())
        .await
        .unwrap();
    assert_eq!(unchanged, Some(row), "empty patch returns the current row");

    let missing_id = PatchTargetId::from(i64::MAX);
    let empty_missing = write::update(pool, missing_id, UpdatePatchTarget::default())
        .await
        .unwrap();
    assert_eq!(empty_missing, None);
    let missing = write::update(
        pool,
        missing_id,
        UpdatePatchTarget {
            score: Some(1),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(missing, None, "updating a missing row returns None");
}

async fn delete_reports_existing_and_missing_rows(pool: &PgPool) {
    use db::patch_targets::{read, write};

    let row = write::create(pool, base_patch_target()).await.unwrap();
    assert!(read::exists_by_id(pool, row.id).await.unwrap());
    assert!(
        write::delete(pool, row.id).await.unwrap(),
        "existing row is deleted"
    );
    assert!(
        !write::delete(pool, row.id).await.unwrap(),
        "second delete finds nothing"
    );
    assert!(!read::exists_by_id(pool, row.id).await.unwrap());
    assert_eq!(read::get_by_id(pool, row.id).await.unwrap(), None);
}

async fn transactions_stay_with_the_caller(pool: &PgPool) {
    use db::patch_targets::{read, write};

    let mut tx = pool.begin().await.unwrap();
    let rolled_back = write::create(&mut *tx, base_patch_target()).await.unwrap();
    assert!(read::exists_by_id(&mut *tx, rolled_back.id).await.unwrap());
    tx.rollback().await.unwrap();
    assert!(!read::exists_by_id(pool, rolled_back.id).await.unwrap());

    let mut tx = pool.begin().await.unwrap();
    let committed = write::create(&mut *tx, base_patch_target()).await.unwrap();
    let patch = UpdatePatchTarget {
        label: Some("in tx".to_owned()),
        ..Default::default()
    };
    write::update(&mut *tx, committed.id, patch).await.unwrap();
    tx.commit().await.unwrap();
    let stored = read::get_by_id(pool, committed.id).await.unwrap().unwrap();
    assert_eq!(stored.label, "in tx");
}

async fn all_defaulted_insert_uses_default_values(pool: &PgPool) {
    use db::counters::models::NewCounter;
    use db::counters::write::create;

    let empty = create(pool, NewCounter::default()).await.unwrap();
    assert_eq!(empty.hits, 0);
    let set = create(
        pool,
        NewCounter {
            hits: Some(3),
            created_at: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(set.hits, 3);
    assert!(set.id > empty.id);
}

async fn reserved_and_quoted_identifiers_round_trip(pool: &PgPool) {
    use db::reserved_words::models::{NewReservedWord, UpdateReservedWord};
    use db::reserved_words::{read, write};

    let row = write::create(
        pool,
        NewReservedWord {
            from: "origin".to_owned(),
            type_: Some("kind".to_owned()),
            mixed_case: None,
            user: None,
            _1st: Some(true),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        row.mixed_case, 1,
        "quoted defaulted column applies its default"
    );
    assert_eq!(row.type_.as_deref(), Some("kind"));
    assert_eq!(row._1st, Some(true));

    let patch = UpdateReservedWord {
        type_: Some(None),
        user: Some(Some("alice".to_owned())),
        mixed_case: Some(9),
        ..Default::default()
    };
    let updated = write::update(pool, row.select, patch)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.type_, None);
    assert_eq!(updated.user.as_deref(), Some("alice"));
    assert_eq!(updated.mixed_case, 9);
    assert_eq!(updated.from, "origin");

    assert_eq!(read::count(pool).await.unwrap(), 1);
    assert_eq!(read::list_page(pool, 10, 0).await.unwrap(), vec![updated]);
    assert!(write::delete(pool, row.select).await.unwrap());
}

async fn table_without_primary_key(pool: &PgPool) {
    use db::audit_log::models::NewAuditLog;
    use db::audit_log::{read, write};

    for event in ["first", "second"] {
        write::create(
            pool,
            NewAuditLog {
                event: event.to_owned(),
                recorded_at: None,
            },
        )
        .await
        .unwrap();
    }
    assert_eq!(read::count(pool).await.unwrap(), 2);
    let mut events: Vec<String> = read::list(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.event)
        .collect();
    events.sort();
    assert_eq!(events, ["first", "second"]);
}

async fn uuid_keys_listing_and_pagination(pool: &PgPool) {
    use db::categories::models::NewCategory;
    use db::products::models::NewProduct;
    use db::products::{read, write};

    let category = db::categories::write::create(
        pool,
        NewCategory {
            name: "tools".to_owned(),
            description: None,
        },
    )
    .await
    .unwrap();

    // Inserted out of key order; list and list_page must return key order.
    for (index, id) in [3u128, 1, 4, 2, 5].into_iter().enumerate() {
        let product = write::create(
            pool,
            NewProduct {
                id: uuid::Uuid::from_u128(id).into(),
                sku: format!("SKU-{id}"),
                name: format!("Product {id}"),
                category_id: category.id,
                supplier_id: None,
                price_cents: (index == 0).then_some(1999),
                stock_on_hand: None,
                reorder_level: None,
                weight_kg: Some(1.5),
                discontinued: None,
                released_on: None,
                created_at: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            product.reorder_level, 10,
            "defaulted SMALLINT applies DEFAULT 10"
        );
    }

    let all: Vec<u128> = read::list(pool)
        .await
        .unwrap()
        .iter()
        .map(|row| uuid::Uuid::from(row.id).as_u128())
        .collect();
    assert_eq!(all, [1, 2, 3, 4, 5]);

    let mut paged = Vec::new();
    for page in 0..3 {
        let rows = read::list_page(pool, 2, page * 2).await.unwrap();
        paged.extend(rows.iter().map(|row| uuid::Uuid::from(row.id).as_u128()));
    }
    assert_eq!(paged, all, "pages neither repeat nor skip rows");
    assert_eq!(read::count(pool).await.unwrap(), 5);

    let first = read::get_by_id(pool, uuid::Uuid::from_u128(3).into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.price_cents, 1999);
    assert_eq!(first.weight_kg, Some(1.5));
}

async fn composite_keys_arrays_and_numeric(pool: &PgPool) {
    use db::feature_line_items::models::{
        FeatureLineItemKey, NewFeatureLineItem, UpdateFeatureLineItem,
    };
    use db::feature_line_items::{read, write};

    let order_id = uuid::Uuid::from_u128(10);
    let product_id = uuid::Uuid::from_u128(20);
    let row = write::create(
        pool,
        NewFeatureLineItem {
            order_id,
            product_id,
            labels: vec!["a".to_owned(), "b".to_owned()],
            weekdays: Some(vec![1, 5]),
            reporter_ids: None,
            cut_off: chrono::NaiveTime::from_hms_opt(17, 30, 0),
            unit_price: Decimal::new(1999, 2),
        },
    )
    .await
    .unwrap();
    assert_eq!(row.unit_price, Decimal::new(1999, 2));

    let patch = UpdateFeatureLineItem {
        weekdays: Some(None),
        unit_price: Some(Decimal::new(2500, 2)),
        ..Default::default()
    };
    let key = FeatureLineItemKey {
        order_id,
        product_id,
    };
    assert_eq!(row.key(), key, "a row hands back its key");
    let updated = write::update(pool, key, patch).await.unwrap().unwrap();
    assert_eq!(updated.weekdays, None);
    assert_eq!(updated.unit_price, Decimal::new(2500, 2));
    assert_eq!(updated.labels, ["a", "b"]);
    assert_eq!(read::get_by_id(pool, key).await.unwrap(), Some(updated));

    let swapped = FeatureLineItemKey {
        order_id: product_id,
        product_id: order_id,
    };
    assert!(read::exists_by_id(pool, key).await.unwrap());
    assert!(
        !read::exists_by_id(pool, swapped).await.unwrap(),
        "key parts are not interchangeable"
    );
    assert!(write::delete(pool, key).await.unwrap());
    assert!(!read::exists_by_id(pool, key).await.unwrap());
}

/// `revised_notes` is created in extra_schema.sql and altered in
/// alter_schema.sql; the generated model carries only the final columns.
async fn altered_table_has_its_final_columns(pool: &PgPool) {
    use db::revised_notes::models::{NewRevisedNote, RevisedNote};
    use db::revised_notes::{read, write};

    let created = write::create(
        pool,
        NewRevisedNote {
            headline: "renamed".to_owned(),
            body: Some("added".to_owned()),
        },
    )
    .await
    .unwrap();
    let defaulted = write::create(
        pool,
        NewRevisedNote {
            headline: "default body".to_owned(),
            body: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(defaulted.body, "");

    let stored = read::get_by_id(pool, created.id).await.unwrap().unwrap();
    assert_eq!(
        stored,
        RevisedNote {
            id: created.id,
            headline: "renamed".to_owned(),
            body: "added".to_owned(),
        }
    );
}

/// `ticket_priority` and `app.ticket_state` (extended by alter_schema.sql)
/// are database enums; their columns use the generated `db::types` enums in
/// rows, inputs, patches, filters, sort keys, arrays, and query files.
async fn enums_keep_their_exact_labels(pool: &PgPool) {
    use db::queries::tickets::tickets_with_priority;
    use db::search::{Equality, Page, Sort};
    use db::tickets::filter::{TicketFilter, TicketSortKey};
    use db::tickets::models::{NewTicket, Ticket, UpdateTicket};
    use db::tickets::{read, write};
    use db::types::{TicketPriority, TicketState};

    let urgent = write::create(
        pool,
        NewTicket {
            priority: TicketPriority::VeryHigh,
            state: Some(TicketState::Closed),
            watch_levels: Some(vec![TicketPriority::Low, TicketPriority::Medium]),
        },
    )
    .await
    .unwrap();
    let routine = write::create(
        pool,
        NewTicket {
            priority: TicketPriority::High,
            state: None,
            watch_levels: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(routine.watch_levels, [], "the array default applies");

    let stored = read::get_by_id(pool, urgent.id).await.unwrap().unwrap();
    assert_eq!(
        stored,
        Ticket {
            id: urgent.id,
            priority: TicketPriority::VeryHigh,
            state: Some(TicketState::Closed),
            watch_levels: vec![TicketPriority::Low, TicketPriority::Medium],
        }
    );
    let labels: (String, Option<String>, Vec<String>) = sqlx::query_as(
        "SELECT priority::text, state::text, watch_levels::text[] FROM app.tickets WHERE id = $1",
    )
    .bind(urgent.id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        labels,
        (
            "Very High".to_owned(),
            Some("CLOSED".to_owned()),
            vec!["Low".to_owned(), "MEDIUM".to_owned()],
        ),
        "the database stores each variant's exact label"
    );

    // `Archived` exists only because of ALTER TYPE ... ADD VALUE.
    let archived = write::update(
        pool,
        urgent.id,
        UpdateTicket {
            state: Some(Some(TicketState::Archived)),
            watch_levels: Some(vec![TicketPriority::High]),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(archived.state, Some(TicketState::Archived));
    assert_eq!(archived.watch_levels, [TicketPriority::High]);

    let very_high = read::search(
        pool,
        TicketFilter {
            priority: vec![Equality::Eq(TicketPriority::VeryHigh)],
            ..Default::default()
        },
        Sort::asc(TicketSortKey::Id),
        Page::first(10),
    )
    .await
    .unwrap();
    assert_eq!(very_high, std::slice::from_ref(&archived));

    let not_archived = read::search(
        pool,
        TicketFilter {
            state: vec![Equality::NotIn(vec![TicketState::Archived])],
            ..Default::default()
        },
        Sort::asc(TicketSortKey::Id),
        Page::first(10),
    )
    .await
    .unwrap();
    assert!(not_archived.is_empty(), "a NULL state matches no condition");

    // Enums sort in declaration order: `high` comes before `Very High`.
    let by_priority = read::search(
        pool,
        TicketFilter::default(),
        Sort::desc(TicketSortKey::Priority),
        Page::first(10),
    )
    .await
    .unwrap();
    let priorities: Vec<TicketPriority> =
        by_priority.iter().map(|ticket| ticket.priority).collect();
    assert_eq!(priorities, [TicketPriority::VeryHigh, TicketPriority::High]);

    let found = tickets_with_priority(pool, TicketPriority::High)
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, routine.id);
    assert_eq!(found[0].priority, TicketPriority::High);
    assert_eq!(found[0].state, None);
}

/// `app.shipments.status`, `app.shipments.carrier`, `refunds.status`, and
/// `refunds.reason` (added by alter_schema.sql) are value-list columns: text
/// columns restricted by `CHECK (col IN (...))`. Each becomes its own enum in
/// `db::types`, encoded as text with each exact value.
async fn value_list_columns_round_trip(pool: &PgPool) {
    use db::queries::shipments::shipments_with_status;
    use db::refunds::models::NewRefund;
    use db::search::{Equality, Page, Sort};
    use db::shipments::filter::{ShipmentFilter, ShipmentSortKey};
    use db::shipments::models::{NewShipment, Shipment, UpdateShipment};
    use db::shipments::{read, write};
    use db::types::{RefundReason, RefundStatus, ShipmentCarrier, ShipmentStatus};

    let defaulted = write::create(
        pool,
        NewShipment {
            status: None,
            carrier: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        defaulted.status,
        ShipmentStatus::Pending,
        "the default applies"
    );
    assert_eq!(defaulted.carrier, None);

    let moving = write::create(
        pool,
        NewShipment {
            status: Some(ShipmentStatus::InTransit),
            carrier: Some(ShipmentCarrier::Dhl),
        },
    )
    .await
    .unwrap();
    let stored = read::get_by_id(pool, moving.id).await.unwrap().unwrap();
    assert_eq!(
        stored,
        Shipment {
            id: moving.id,
            status: ShipmentStatus::InTransit,
            carrier: Some(ShipmentCarrier::Dhl),
        }
    );
    let text: (String, Option<String>) =
        sqlx::query_as("SELECT status, carrier FROM app.shipments WHERE id = $1")
            .bind(moving.id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(
        text,
        ("In Transit".to_owned(), Some("dhl".to_owned())),
        "the database stores each variant's exact value"
    );

    let delivered = write::update(
        pool,
        moving.id,
        UpdateShipment {
            status: Some(ShipmentStatus::Delivered),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(delivered.status, ShipmentStatus::Delivered);
    assert_eq!(ShipmentStatus::Delivered.as_str(), "DELIVERED");

    let found = read::search(
        pool,
        ShipmentFilter {
            status: vec![Equality::In(vec![
                ShipmentStatus::Delivered,
                ShipmentStatus::InTransit,
            ])],
            carrier: vec![Equality::Eq(ShipmentCarrier::Dhl)],
            ..Default::default()
        },
        Sort::asc(ShipmentSortKey::Status),
        Page::first(10),
    )
    .await
    .unwrap();
    assert_eq!(found, std::slice::from_ref(&delivered));

    let pending = shipments_with_status(pool, ShipmentStatus::Pending)
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, defaulted.id);
    assert_eq!(pending[0].status, ShipmentStatus::Pending);

    // Only the listed values exist in Rust: text outside the list neither
    // parses nor decodes.
    assert!("delivered".parse::<ShipmentStatus>().is_err());
    assert_eq!(
        "In Transit".parse::<ShipmentStatus>(),
        Ok(ShipmentStatus::InTransit)
    );
    let decoded = sqlx::query_scalar::<_, ShipmentStatus>("SELECT 'lost'::text")
        .fetch_one(pool)
        .await;
    assert!(decoded.is_err(), "an unknown value does not decode");

    // Same column name, different table: a distinct enum.
    let refund = db::refunds::write::create(
        pool,
        NewRefund {
            status: RefundStatus::Requested,
            reason: Some(RefundReason::Late),
        },
    )
    .await
    .unwrap();
    assert_eq!(refund.status, RefundStatus::Requested);
    assert_eq!(refund.reason, Some(RefundReason::Late));
}

/// Every unique key other than the primary key gets a `get_by_<cols>`
/// lookup: `products.sku` (made unique by alter_schema.sql) and the
/// composite `(organization_id, code)` of `app.branches`.
async fn unique_key_lookups_find_one_row_or_none(pool: &PgPool) {
    use db::branches::models::NewBranch;
    use db::categories::models::NewCategory;
    use db::products::models::{NewProduct, ProductId};

    let category = db::categories::write::create(
        pool,
        NewCategory {
            name: "unique".to_owned(),
            description: None,
        },
    )
    .await
    .unwrap();
    for (n, sku) in [(8_001, "U-1"), (8_002, "U-2")] {
        db::products::write::create(
            pool,
            NewProduct {
                id: ProductId::from(uuid::Uuid::from_u128(n)),
                sku: sku.to_owned(),
                name: format!("Unique {sku}"),
                category_id: category.id,
                supplier_id: None,
                price_cents: None,
                stock_on_hand: None,
                reorder_level: None,
                weight_kg: None,
                discontinued: None,
                released_on: None,
                created_at: None,
            },
        )
        .await
        .unwrap();
    }
    let by_sku = db::products::read::get_by_sku(pool, "U-2").await.unwrap();
    assert_eq!(
        by_sku.map(|row| row.id),
        Some(ProductId::from(uuid::Uuid::from_u128(8_002)))
    );
    assert_eq!(
        db::products::read::get_by_sku(pool, "U-3").await.unwrap(),
        None
    );

    let acme = uuid::Uuid::from_u128(1);
    let globex = uuid::Uuid::from_u128(2);
    for (organization_id, code, name) in [
        (acme, "HQ", "Acme head office"),
        (globex, "HQ", "Globex head office"),
        (acme, "WEST", "Acme west"),
    ] {
        db::branches::write::create(
            pool,
            NewBranch {
                organization_id,
                code: code.to_owned(),
                name: name.to_owned(),
            },
        )
        .await
        .unwrap();
    }
    let globex_hq = db::branches::read::get_by_organization_id_and_code(pool, globex, "HQ")
        .await
        .unwrap();
    assert_eq!(
        globex_hq.map(|row| row.name),
        Some("Globex head office".to_owned())
    );
    let acme_west = db::branches::read::get_by_organization_id_and_code(pool, acme, "WEST")
        .await
        .unwrap();
    assert_eq!(acme_west.map(|row| row.name), Some("Acme west".to_owned()));
    let globex_west = db::branches::read::get_by_organization_id_and_code(pool, globex, "WEST")
        .await
        .unwrap();
    assert_eq!(globex_west, None);
}

/// `touched_notes` is generated with `updated_at` and `row_version` managed:
/// neither appears in `NewTouchedNote` or `UpdateTouchedNote` (the struct
/// literals below name every field), and the rows carry the values the
/// default and the `touch_row` trigger give them.
async fn managed_columns_are_left_to_the_database(pool: &PgPool) {
    use db::touched_notes::models::{NewTouchedNote, UpdateTouchedNote};
    use db::touched_notes::{read, write};

    let created = write::create(
        pool,
        NewTouchedNote {
            body: "first".to_owned(),
        },
    )
    .await
    .unwrap();
    assert_eq!(created.row_version, 1, "the column default applies");

    let updated = write::update(
        pool,
        created.id,
        UpdateTouchedNote {
            body: Some("second".to_owned()),
        },
    )
    .await
    .unwrap()
    .expect("row exists");
    assert_eq!(updated.body, "second");
    assert_eq!(updated.row_version, 2, "the trigger advanced the version");
    assert!(
        updated.updated_at > created.updated_at,
        "the trigger touched updated_at"
    );

    let stored = read::get_by_id(pool, created.id).await.unwrap();
    assert_eq!(stored, Some(updated));
}

/// `versioned_documents` is generated with `revision` as its version column:
/// `update` takes the revision the caller last saw, and the `bump_revision`
/// trigger advances it on every update. `revision` is managed, so neither
/// `NewVersionedDocument` nor `UpdateVersionedDocument` has it (the struct
/// literals below name every field).
async fn versioned_updates_detect_concurrent_edits(pool: &PgPool) {
    use db::UpdateOutcome;
    use db::search::{Page, Sort};
    use db::versioned_documents::filter::{VersionedDocumentFilter, VersionedDocumentSortKey};
    use db::versioned_documents::models::{
        NewVersionedDocument, UpdateVersionedDocument, VersionedDocumentId,
    };
    use db::versioned_documents::{read, write};

    let created = write::create(
        pool,
        NewVersionedDocument {
            title: "draft".to_owned(),
            note: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(created.revision, 1);

    // Two editors both saw revision 1; the first one to save wins.
    let first = write::update(
        pool,
        created.id,
        1,
        UpdateVersionedDocument {
            title: Some("first editor".to_owned()),
            note: None,
        },
    )
    .await
    .unwrap();
    let UpdateOutcome::Updated(saved) = first else {
        panic!("the first update should apply, got {first:?}");
    };
    assert_eq!(saved.title, "first editor");
    assert_eq!(saved.revision, 2, "the trigger advanced the revision");

    let second = write::update(
        pool,
        created.id,
        1,
        UpdateVersionedDocument {
            title: Some("second editor".to_owned()),
            note: Some(Some("stale".to_owned())),
        },
    )
    .await
    .unwrap();
    assert_eq!(second, UpdateOutcome::VersionConflict);
    let stored = read::get_by_id(pool, created.id).await.unwrap();
    assert_eq!(
        stored,
        Some(saved.clone()),
        "the stale update changed nothing"
    );

    // Inside a caller-owned transaction, with the current revision.
    let mut tx = pool.begin().await.unwrap();
    let retried = write::update(
        &mut *tx,
        created.id,
        2,
        UpdateVersionedDocument {
            title: Some("second editor".to_owned()),
            note: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let UpdateOutcome::Updated(retried) = retried else {
        panic!("an update at the current revision should apply, got {retried:?}");
    };
    assert_eq!(retried.revision, 3);

    // An empty patch returns the current row without bumping the revision,
    // and still reports a stale revision as a conflict.
    let unchanged = write::update(pool, created.id, 3, UpdateVersionedDocument::default())
        .await
        .unwrap();
    assert_eq!(unchanged, UpdateOutcome::Updated(retried.clone()));
    let stale_empty = write::update(pool, created.id, 2, UpdateVersionedDocument::default())
        .await
        .unwrap();
    assert_eq!(stale_empty, UpdateOutcome::VersionConflict);
    let stored = read::get_by_id(pool, created.id).await.unwrap();
    assert_eq!(stored.map(|row| row.revision), Some(3));

    // A key no row has is not found, with or without a patch.
    let missing = VersionedDocumentId::from(i64::from(created.id) + 1_000);
    let patched_missing = write::update(
        pool,
        missing,
        1,
        UpdateVersionedDocument {
            title: Some("ghost".to_owned()),
            note: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(patched_missing, UpdateOutcome::NotFound);
    let empty_missing = write::update(pool, missing, 1, UpdateVersionedDocument::default())
        .await
        .unwrap();
    assert_eq!(empty_missing, UpdateOutcome::NotFound);

    // A soft-deleted row counts as missing, even at its current revision,
    // and the update leaves it unchanged.
    assert!(write::delete(pool, created.id).await.unwrap());
    let deleted = read::search(
        pool,
        VersionedDocumentFilter {
            include_deleted: true,
            ..VersionedDocumentFilter::default()
        },
        Sort::asc(VersionedDocumentSortKey::Id),
        Page::first(10),
    )
    .await
    .unwrap()
    .remove(0);
    assert!(deleted.deleted_at.is_some());
    let patched_deleted = write::update(
        pool,
        created.id,
        deleted.revision,
        UpdateVersionedDocument {
            title: Some("revived".to_owned()),
            note: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(patched_deleted, UpdateOutcome::NotFound);
    let empty_deleted = write::update(
        pool,
        created.id,
        deleted.revision,
        UpdateVersionedDocument::default(),
    )
    .await
    .unwrap();
    assert_eq!(empty_deleted, UpdateOutcome::NotFound);
    assert!(write::restore(pool, created.id).await.unwrap());
    let restored = read::get_by_id(pool, created.id).await.unwrap().unwrap();
    assert_eq!(
        restored.title, "second editor",
        "the update changed nothing"
    );
}

/// A text key's typed id is not `Copy`, so every generated function borrows
/// it; a key struct with a text column is borrowed the same way.
async fn text_keys_are_borrowed(pool: &PgPool) {
    use db::currencies::models::{CurrencyId, NewCurrency, UpdateCurrency};
    use db::currency_rates::models::{CurrencyRateKey, NewCurrencyRate};
    use db::search::Limit;

    let eur = CurrencyId::from("EUR".to_owned());
    for code in ["CHF", "EUR"] {
        db::currencies::write::create(
            pool,
            NewCurrency {
                code: CurrencyId::from(code.to_owned()),
                name: code.to_lowercase(),
            },
        )
        .await
        .unwrap();
    }

    let found = db::currencies::read::get_by_id(pool, &eur).await.unwrap();
    assert_eq!(found.map(|row| row.name), Some("eur".to_owned()));
    assert!(
        db::currencies::read::exists_by_id(pool, &eur)
            .await
            .unwrap()
    );
    let renamed = db::currencies::write::update(
        pool,
        &eur,
        UpdateCurrency {
            name: Some("Euro".to_owned()),
        },
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(renamed.name, "Euro");

    let first = db::currencies::read::search_after(pool, Default::default(), None, Limit::new(1))
        .await
        .unwrap();
    assert_eq!(first[0].code, CurrencyId::from("CHF".to_owned()));
    let next = db::currencies::read::search_after(
        pool,
        Default::default(),
        Some(&first[0].code),
        Limit::new(10),
    )
    .await
    .unwrap();
    assert_eq!(next.iter().map(|row| &row.code).collect::<Vec<_>>(), [&eur]);

    let day = chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
    let rate = db::currency_rates::write::create(
        pool,
        NewCurrencyRate {
            currency_code: eur.clone(),
            day,
            rate: Decimal::new(1_085_000, 6),
        },
    )
    .await
    .unwrap();
    // Query files take and return the typed id too.
    let name = db::queries::currencies::currency_name(pool, &eur)
        .await
        .unwrap();
    assert_eq!(name.as_deref(), Some("Euro"));
    let rates = db::queries::currencies::rates_for_currencies(pool, std::slice::from_ref(&eur))
        .await
        .unwrap();
    assert_eq!(rates, std::slice::from_ref(&rate));
    assert_eq!(rates[0].currency_code, eur);

    let key: CurrencyRateKey = rate.key();
    let fetched = db::currency_rates::read::get_by_id(pool, &key)
        .await
        .unwrap();
    assert_eq!(fetched, Some(rate));
    assert!(db::currency_rates::write::delete(pool, &key).await.unwrap());
    assert!(db::currencies::write::delete(pool, &eur).await.unwrap());
}

/// The fixture generates with `derive = ["serde"]`: rows read from the
/// database serialize to JSON and back unchanged. Typed ids serialize as
/// their inner value and enum variants as their database label.
async fn rows_round_trip_through_json(pool: &PgPool) {
    use db::products::models::Product;
    use db::tickets::models::Ticket;

    let products = db::products::read::list(pool).await.unwrap();
    assert!(!products.is_empty(), "earlier scenarios left products");
    for product in &products {
        let json = serde_json::to_string(product).unwrap();
        assert_eq!(&serde_json::from_str::<Product>(&json).unwrap(), product);
    }
    let value = serde_json::to_value(&products[0]).unwrap();
    assert_eq!(value["id"], serde_json::json!(products[0].id.0));

    let tickets = db::tickets::read::list(pool).await.unwrap();
    let very_high = tickets
        .iter()
        .find(|ticket| ticket.priority == db::types::TicketPriority::VeryHigh)
        .expect("earlier scenarios left a very high ticket");
    let json = serde_json::to_value(very_high).unwrap();
    assert_eq!(json["priority"], "Very High");
    assert_eq!(&serde_json::from_value::<Ticket>(json).unwrap(), very_high);
}

/// `soft_notes` is generated with `deleted_at` as its soft-delete column:
/// `NewSoftNote` and `UpdateSoftNote` have no `deleted_at` (the struct
/// literals below name every field), `delete` hides a row from every read,
/// `restore` brings it back, and `delete_permanently` removes it.
async fn soft_deleted_rows_are_hidden_restored_and_removed(pool: &PgPool) {
    use db::search::{Limit, Page, Sort};
    use db::soft_notes::filter::{SoftNoteFilter, SoftNoteSortKey};
    use db::soft_notes::models::{NewSoftNote, SoftNote, UpdateSoftNote};
    use db::soft_notes::{read, write};

    let create = |code: &str| {
        write::create(
            pool,
            NewSoftNote {
                code: code.to_owned(),
                body: format!("note {code}"),
            },
        )
    };
    let note = create("A").await.unwrap();
    let other = create("B").await.unwrap();
    assert_eq!(note.deleted_at, None);

    let ids = |rows: Vec<SoftNote>| rows.into_iter().map(|row| row.id).collect::<Vec<_>>();
    let search = |filter: SoftNoteFilter| {
        read::search(
            pool,
            filter,
            Sort::asc(SoftNoteSortKey::Id),
            Page::first(10),
        )
    };
    let assert_visible = async |visible: bool| {
        let expect = |ids: Vec<_>| {
            if visible {
                assert_eq!(ids, vec![note.id, other.id]);
            } else {
                assert_eq!(ids, vec![other.id]);
            }
        };
        assert_eq!(
            read::get_by_id(pool, note.id).await.unwrap().is_some(),
            visible
        );
        assert_eq!(
            read::get_by_code(pool, "A").await.unwrap().is_some(),
            visible
        );
        assert_eq!(read::exists_by_id(pool, note.id).await.unwrap(), visible);
        expect(ids(read::list(pool).await.unwrap()));
        expect(ids(read::list_page(pool, 10, 0).await.unwrap()));
        assert_eq!(
            read::count(pool).await.unwrap(),
            if visible { 2 } else { 1 }
        );
        expect(ids(search(SoftNoteFilter::default()).await.unwrap()));
        expect(ids(read::search_after(
            pool,
            SoftNoteFilter::default(),
            None,
            Limit::new(10),
        )
        .await
        .unwrap()));
    };
    assert_visible(true).await;

    assert!(write::delete(pool, note.id).await.unwrap());
    assert!(
        !write::delete(pool, note.id).await.unwrap(),
        "an already-deleted row does not change"
    );
    assert_visible(false).await;
    let patch = UpdateSoftNote {
        code: None,
        body: Some("edited".to_owned()),
    };
    assert_eq!(write::update(pool, note.id, patch).await.unwrap(), None);
    let empty = UpdateSoftNote {
        code: None,
        body: None,
    };
    assert_eq!(write::update(pool, note.id, empty).await.unwrap(), None);

    let with_deleted = search(SoftNoteFilter {
        include_deleted: true,
        ..SoftNoteFilter::default()
    })
    .await
    .unwrap();
    assert_eq!(ids(with_deleted.clone()), vec![note.id, other.id]);
    assert!(with_deleted[0].deleted_at.is_some());
    assert_eq!(with_deleted[0].body, "note A", "the update changed nothing");

    assert!(write::restore(pool, note.id).await.unwrap());
    assert!(
        !write::restore(pool, note.id).await.unwrap(),
        "already live"
    );
    assert_visible(true).await;

    // The partial unique index holds among live rows only: a deleted row and
    // a live row share a code, and `get_by_code` finds the live one.
    assert!(write::delete(pool, note.id).await.unwrap());
    let replacement = create("A").await.unwrap();
    assert_eq!(
        read::get_by_code(pool, "A")
            .await
            .unwrap()
            .map(|row| row.id),
        Some(replacement.id)
    );

    assert!(write::delete_permanently(pool, note.id).await.unwrap());
    assert!(!write::delete_permanently(pool, note.id).await.unwrap());
    let remaining = search(SoftNoteFilter {
        include_deleted: true,
        ..SoftNoteFilter::default()
    })
    .await
    .unwrap();
    assert_eq!(ids(remaining), vec![other.id, replacement.id]);
}

/// `tenant_notes` is scoped by `tenant_id`: every generated function takes
/// the tenant right after the executor, so a row created under one tenant is
/// invisible to every read, update, and delete made with another. The struct
/// literals below name every field, so `NewTenantNote` and `UpdateTenantNote`
/// have no `tenant_id`.
async fn tenant_rows_are_invisible_to_other_tenants(pool: &PgPool) {
    use db::UpdateOutcome;
    use db::search::{Limit, Page, Sort};
    use db::tenant_notes::filter::{TenantNoteFilter, TenantNoteSortKey};
    use db::tenant_notes::models::{NewTenantNote, TenantNote, UpdateTenantNote};
    use db::tenant_notes::{read, write};

    let tenant_a = uuid::Uuid::from_u128(18_001);
    let tenant_b = uuid::Uuid::from_u128(18_002);
    let create = |tenant, body: &str| {
        write::create(
            pool,
            tenant,
            NewTenantNote {
                code: "N".to_owned(),
                body: body.to_owned(),
            },
        )
    };
    let note = create(tenant_a, "a").await.unwrap();
    // The unique key is per tenant, so both tenants can use the same code.
    let other = create(tenant_b, "b").await.unwrap();
    assert_eq!(
        note.tenant_id, tenant_a,
        "the tenant comes from the parameter"
    );
    assert_eq!(other.tenant_id, tenant_b);

    let ids = |rows: Vec<TenantNote>| rows.into_iter().map(|row| row.id).collect::<Vec<_>>();
    let sees = async |tenant, expected: Vec<_>| {
        assert_eq!(ids(read::list(pool, tenant).await.unwrap()), expected);
        assert_eq!(
            ids(read::list_page(pool, tenant, 10, 0).await.unwrap()),
            expected
        );
        assert_eq!(
            read::count(pool, tenant).await.unwrap(),
            i64::try_from(expected.len()).unwrap()
        );
        let search = read::search(
            pool,
            tenant,
            TenantNoteFilter::default(),
            Sort::asc(TenantNoteSortKey::Id),
            Page::first(10),
        )
        .await
        .unwrap();
        assert_eq!(ids(search), expected);
        let after = read::search_after(
            pool,
            tenant,
            TenantNoteFilter::default(),
            None,
            Limit::new(10),
        )
        .await
        .unwrap();
        assert_eq!(ids(after), expected);
        assert_eq!(
            read::get_by_code(pool, tenant, "N")
                .await
                .unwrap()
                .map(|row| row.id),
            expected.first().copied()
        );
    };
    sees(tenant_a, vec![note.id]).await;
    sees(tenant_b, vec![other.id]).await;

    // Tenant B cannot reach tenant A's row by its key.
    assert_eq!(
        read::get_by_id(pool, tenant_b, note.id).await.unwrap(),
        None
    );
    assert!(!read::exists_by_id(pool, tenant_b, note.id).await.unwrap());
    let patch = || UpdateTenantNote {
        code: None,
        body: Some("hijacked".to_owned()),
    };
    assert_eq!(
        write::update(pool, tenant_b, note.id, 1, patch())
            .await
            .unwrap(),
        UpdateOutcome::NotFound
    );
    let empty = UpdateTenantNote {
        code: None,
        body: None,
    };
    assert_eq!(
        write::update(pool, tenant_b, note.id, 1, empty)
            .await
            .unwrap(),
        UpdateOutcome::NotFound
    );
    assert!(!write::delete(pool, tenant_b, note.id).await.unwrap());
    assert!(
        !write::delete_permanently(pool, tenant_b, note.id)
            .await
            .unwrap()
    );
    assert_eq!(
        read::get_by_id(pool, tenant_a, note.id).await.unwrap(),
        Some(note.clone()),
        "tenant B changed nothing"
    );

    // Tenant A can, and tenant B still cannot restore what A deleted.
    let UpdateOutcome::Updated(updated) = write::update(pool, tenant_a, note.id, 1, patch())
        .await
        .unwrap()
    else {
        panic!("tenant A updates its own row");
    };
    assert_eq!(updated.body, "hijacked");
    assert!(write::delete(pool, tenant_a, note.id).await.unwrap());
    assert!(!write::restore(pool, tenant_b, note.id).await.unwrap());
    assert!(write::restore(pool, tenant_a, note.id).await.unwrap());
    sees(tenant_a, vec![note.id]).await;
    sees(tenant_b, vec![other.id]).await;
}

/// `tenant_members` has the tenant column in its primary key, so its key is
/// `code` alone and the tenant is passed once, as the tenant parameter.
async fn a_tenant_in_the_primary_key_is_passed_once(pool: &PgPool) {
    use db::search::Limit;
    use db::tenant_members::filter::TenantMemberFilter;
    use db::tenant_members::models::{NewTenantMember, UpdateTenantMember};
    use db::tenant_members::{read, write};

    let tenant_a = uuid::Uuid::from_u128(18_011);
    let tenant_b = uuid::Uuid::from_u128(18_012);
    for (tenant, name) in [(tenant_a, "Ann"), (tenant_b, "Bob")] {
        write::create(
            pool,
            tenant,
            NewTenantMember {
                code: "M1".to_owned(),
                name: name.to_owned(),
            },
        )
        .await
        .unwrap();
    }
    let name = async |tenant| {
        read::get_by_id(pool, tenant, "M1")
            .await
            .unwrap()
            .map(|row| row.name)
    };
    assert_eq!(name(tenant_a).await.as_deref(), Some("Ann"));
    assert_eq!(name(tenant_b).await.as_deref(), Some("Bob"));

    let renamed = write::update(
        pool,
        tenant_b,
        "M1",
        UpdateTenantMember {
            name: Some("Bea".to_owned()),
        },
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        (renamed.tenant_id, renamed.name.as_str()),
        (tenant_b, "Bea")
    );
    assert_eq!(name(tenant_a).await.as_deref(), Some("Ann"));

    let page = read::search_after(
        pool,
        tenant_a,
        TenantMemberFilter::default(),
        Some("M0"),
        Limit::new(10),
    )
    .await
    .unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].tenant_id, tenant_a);

    assert!(write::delete(pool, tenant_a, "M1").await.unwrap());
    assert_eq!(name(tenant_a).await, None);
    assert_eq!(name(tenant_b).await.as_deref(), Some("Bea"));
}

/// `rls_notes` has a row-level security policy reading `app.tenant_id`. For a
/// role the policy applies to, rows are visible only after
/// `session::set_tenant` sets the tenant in the transaction; `set_user` sets
/// `app.user_id` the same way.
async fn row_level_security_sees_the_session_tenant(pool: &PgPool) {
    use db::rls_notes::models::NewRlsNote;
    use db::rls_notes::{read, write};
    use db::session;

    // Superusers bypass row-level security, so the reads run as a role the
    // policy applies to. Roles are cluster-wide; it is dropped at the end.
    sqlx::raw_sql(
        "DO $$ BEGIN
             IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'rok_sqlc_fixture_tenant') THEN
                 CREATE ROLE rok_sqlc_fixture_tenant NOLOGIN;
             END IF;
         END $$;
         GRANT USAGE ON SCHEMA public TO rok_sqlc_fixture_tenant;
         GRANT SELECT ON rls_notes TO rok_sqlc_fixture_tenant;",
    )
    .execute(pool)
    .await
    .unwrap();

    let tenant_a = uuid::Uuid::from_u128(18_021);
    let tenant_b = uuid::Uuid::from_u128(18_022);
    let mut created = Vec::new();
    for tenant in [tenant_a, tenant_b] {
        let body = format!("note of {tenant}");
        created.push(
            write::create(pool, tenant, NewRlsNote { body })
                .await
                .unwrap(),
        );
    }

    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql("SET LOCAL ROLE rok_sqlc_fixture_tenant")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        read::list(&mut *tx, tenant_a).await.unwrap(),
        Vec::new(),
        "no tenant setting, so the policy matches nothing"
    );
    session::set_tenant(&mut tx, tenant_a).await.unwrap();
    session::set_user(&mut tx, "user-18").await.unwrap();
    assert_eq!(
        read::list(&mut *tx, tenant_a).await.unwrap(),
        vec![created[0].clone()]
    );
    assert_eq!(read::count(&mut *tx, tenant_b).await.unwrap(), 0);
    let user: String = sqlx::query_scalar("SELECT current_setting('app.user_id')")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(user, "user-18");
    tx.rollback().await.unwrap();

    sqlx::raw_sql("DROP OWNED BY rok_sqlc_fixture_tenant; DROP ROLE rok_sqlc_fixture_tenant;")
        .execute(pool)
        .await
        .unwrap();
}

/// How many times the current transaction has scanned `table` (sequential
/// or index scans), from PostgreSQL's per-transaction statistics. Each
/// statement reading the table scans it once.
async fn scans_in_transaction(tx: &mut sqlx::PgConnection, table: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COALESCE(seq_scan, 0) + COALESCE(idx_scan, 0)
         FROM pg_stat_xact_user_tables WHERE relname = $1",
    )
    .bind(table)
    .fetch_optional(tx)
    .await
    .unwrap()
    .unwrap_or(0)
}

/// Loading `category` for 50 products is one query, keyed by each
/// product's `category_id`.
async fn belongs_to_loader_fetches_every_parent_in_one_query(pool: &PgPool) {
    use db::categories::models::NewCategory;
    use db::products::models::{NewProduct, ProductId};

    let mut categories = Vec::new();
    for name in ["loader-a", "loader-b", "loader-c"] {
        let category = db::categories::write::create(
            pool,
            NewCategory {
                name: name.to_owned(),
                description: None,
            },
        )
        .await
        .unwrap();
        categories.push(category);
    }
    let mut products = Vec::new();
    for index in 0..50_u128 {
        let category = &categories[usize::try_from(index % 3).unwrap()];
        let product = db::products::write::create(
            pool,
            NewProduct {
                id: ProductId::from(uuid::Uuid::from_u128(20_000 + index)),
                sku: format!("L-{index}"),
                name: format!("Loaded {index}"),
                category_id: category.id,
                supplier_id: None,
                price_cents: None,
                stock_on_hand: None,
                reorder_level: None,
                weight_kg: None,
                discontinued: None,
                released_on: None,
                created_at: None,
            },
        )
        .await
        .unwrap();
        products.push(product);
    }

    let mut tx = pool.begin().await.unwrap();
    let before = scans_in_transaction(&mut tx, "categories").await;
    let loaded = db::products::relationships::load_category(&mut *tx, &products)
        .await
        .unwrap();
    let after = scans_in_transaction(&mut tx, "categories").await;
    tx.rollback().await.unwrap();
    assert_eq!(
        after - before,
        1,
        "one query loads every product's category"
    );

    assert_eq!(loaded.len(), 3, "each category is loaded once");
    for product in &products {
        assert_eq!(loaded[&product.category_id].id, product.category_id);
    }
    assert_eq!(loaded[&categories[1].id].name, "loader-b");

    let none = db::products::relationships::load_category(pool, &[])
        .await
        .unwrap();
    assert!(none.is_empty(), "no rows, nothing to load");
}

/// Loading `comments` for posts gives every post an entry (empty when it has
/// no comments) and leaves out soft-deleted comments and other tenants'
/// rows; loading each comment's `post` is scoped the same way.
async fn has_many_loader_stays_in_the_tenant_and_skips_deleted_rows(pool: &PgPool) {
    use db::comments::models::NewComment;
    use db::posts::models::NewPost;
    use db::tenants::models::{NewTenant, TenantId};

    let tenant = async |id: u128| {
        db::tenants::write::create(
            pool,
            NewTenant {
                id: TenantId::from(uuid::Uuid::from_u128(id)),
                name: format!("tenant {id}"),
            },
        )
        .await
        .unwrap()
        .id
    };
    let tenant_a = tenant(20_101).await;
    let tenant_b = tenant(20_102).await;
    let post = async |tenant, title: &str| {
        db::posts::write::create(
            pool,
            tenant,
            NewPost {
                title: title.to_owned(),
            },
        )
        .await
        .unwrap()
    };
    let commented = post(tenant_a, "commented").await;
    let quiet = post(tenant_a, "quiet").await;
    let comment = async |tenant, post_id, body: &str| {
        db::comments::write::create(
            pool,
            tenant,
            NewComment {
                post_id,
                author_id: None,
                body: body.to_owned(),
            },
        )
        .await
        .unwrap()
    };
    let first = comment(tenant_a, commented.id, "first").await;
    let deleted = comment(tenant_a, commented.id, "deleted").await;
    let second = comment(tenant_a, commented.id, "second").await;
    // The foreign key does not stop another tenant's row from pointing here.
    let foreign = comment(tenant_b, commented.id, "foreign").await;
    assert!(
        db::comments::write::delete(pool, tenant_a, deleted.id)
            .await
            .unwrap()
    );

    let posts = vec![commented.clone(), quiet.clone()];
    let loaded = db::posts::relationships::load_comments(pool, tenant_a, &posts)
        .await
        .unwrap();
    assert_eq!(loaded.len(), 2, "every post has an entry");
    assert_eq!(loaded[&commented.id], vec![first.clone(), second.clone()]);
    assert_eq!(loaded[&quiet.id], Vec::new(), "no comments, an empty list");

    let other_tenant = db::posts::relationships::load_comments(pool, tenant_b, &posts)
        .await
        .unwrap();
    assert_eq!(other_tenant[&commented.id], vec![foreign.clone()]);

    let comments = vec![first, foreign];
    let parents = db::comments::relationships::load_post(pool, tenant_a, &comments)
        .await
        .unwrap();
    assert_eq!(parents.len(), 1);
    assert_eq!(parents[&commented.id], commented);
    assert!(
        db::comments::relationships::load_post(pool, tenant_b, &comments)
            .await
            .unwrap()
            .is_empty(),
        "the post belongs to another tenant"
    );

    assert!(
        db::posts::write::delete(pool, tenant_a, commented.id)
            .await
            .unwrap()
    );
    assert!(
        db::comments::relationships::load_post(pool, tenant_a, &comments)
            .await
            .unwrap()
            .is_empty(),
        "the post is soft-deleted"
    );
}

/// `invoices` references `users` twice: each foreign key gets its own
/// belongs-to (`created_by_user`, `approved_by_user`) and its own inverse
/// (`invoices_by_created_by_user`, `invoices_by_approved_by_user`).
async fn several_foreign_keys_to_one_table_get_distinct_names(pool: &PgPool) {
    use db::invoices::models::NewInvoice;
    use db::invoices::relationships::{load_approved_by_user, load_created_by_user};
    use db::users::models::NewUser;
    use db::users::relationships::{
        load_invoices_by_approved_by_user, load_invoices_by_created_by_user,
    };

    let user = async |email: &str| {
        db::users::write::create(
            pool,
            NewUser {
                email: email.to_owned(),
            },
        )
        .await
        .unwrap()
    };
    let clerk = user("clerk@example.com").await;
    let manager = user("manager@example.com").await;
    let invoice = async |approved_by_user_id, total_cents| {
        db::invoices::write::create(
            pool,
            NewInvoice {
                created_by_user_id: clerk.id,
                approved_by_user_id,
                total_cents,
            },
        )
        .await
        .unwrap()
    };
    let approved = invoice(Some(manager.id), 100).await;
    let pending = invoice(None, 200).await;
    let invoices = vec![approved.clone(), pending.clone()];

    let creators = load_created_by_user(pool, &invoices).await.unwrap();
    assert_eq!(creators.len(), 1);
    assert_eq!(creators[&clerk.id], clerk);
    let approvers = load_approved_by_user(pool, &invoices).await.unwrap();
    assert_eq!(approvers.len(), 1, "a NULL foreign key loads nothing");
    assert_eq!(approvers[&manager.id], manager);

    let users = vec![clerk.clone(), manager.clone()];
    let created = load_invoices_by_created_by_user(pool, &users)
        .await
        .unwrap();
    assert_eq!(created[&clerk.id], vec![approved.clone(), pending]);
    assert_eq!(created[&manager.id], Vec::new());
    let to_approve = load_invoices_by_approved_by_user(pool, &users)
        .await
        .unwrap();
    assert_eq!(to_approve[&clerk.id], Vec::new());
    assert_eq!(to_approve[&manager.id], vec![approved]);
}

/// A foreign key on a unique column is a has-one: one row per key.
async fn unique_foreign_key_loads_one_row(pool: &PgPool) {
    use db::user_profiles::models::NewUserProfile;
    use db::users::models::NewUser;

    let mut users = Vec::new();
    for email in ["profiled@example.com", "unprofiled@example.com"] {
        let user = db::users::write::create(
            pool,
            NewUser {
                email: email.to_owned(),
            },
        )
        .await
        .unwrap();
        users.push(user);
    }
    let profile = db::user_profiles::write::create(
        pool,
        NewUserProfile {
            user_id: users[0].id,
            bio: "has a profile".to_owned(),
        },
    )
    .await
    .unwrap();

    let profiles = db::users::relationships::load_user_profile(pool, &users)
        .await
        .unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[&users[0].id], profile);
    let owners = db::user_profiles::relationships::load_user(pool, &[profile])
        .await
        .unwrap();
    assert_eq!(owners[&users[0].id], users[0]);
}

/// A `parent_id` self-reference loads each unit's `parent` and `children`.
async fn self_reference_loads_parent_and_children(pool: &PgPool) {
    use db::org_units::models::NewOrgUnit;
    use db::org_units::relationships::{load_children, load_parent};

    let unit = async |parent_id, name: &str| {
        db::org_units::write::create(
            pool,
            NewOrgUnit {
                parent_id,
                name: name.to_owned(),
            },
        )
        .await
        .unwrap()
    };
    let root = unit(None, "root").await;
    let left = unit(Some(root.id), "left").await;
    let right = unit(Some(root.id), "right").await;
    let leaf = unit(Some(left.id), "leaf").await;

    let units = vec![root.clone(), left.clone(), right.clone(), leaf.clone()];
    let parents = load_parent(pool, &units).await.unwrap();
    assert_eq!(parents.len(), 2, "root and left are parents");
    assert_eq!(parents[&root.id], root);
    assert_eq!(parents[&left.id], left);

    let children = load_children(pool, &units).await.unwrap();
    assert_eq!(children[&root.id], vec![left.clone(), right.clone()]);
    assert_eq!(children[&left.id], vec![leaf.clone()]);
    assert_eq!(children[&right.id], Vec::new());
    assert_eq!(children[&leaf.id], Vec::new());
}

/// Products filtered by their category's name: exactly the products in a
/// matching category, each once.
async fn belongs_to_filter_matches_each_row_once(pool: &PgPool) {
    use db::categories::filter::CategoryFilter;
    use db::categories::models::NewCategory;
    use db::products::filter::{ProductFilter, ProductSortKey};
    use db::products::models::{NewProduct, ProductId};
    use db::search::{Page, Sort, Text};

    let mut categories = Vec::new();
    for name in ["rel-tools", "rel-toys", "rel-tools-extra"] {
        let category = db::categories::write::create(
            pool,
            NewCategory {
                name: name.to_owned(),
                description: None,
            },
        )
        .await
        .unwrap();
        categories.push(category);
    }
    for (index, category) in [0, 0, 1, 2, 0].into_iter().enumerate() {
        db::products::write::create(
            pool,
            NewProduct {
                id: ProductId::from(uuid::Uuid::from_u128(22_000 + index as u128)),
                sku: format!("REL-{index}"),
                name: format!("Related {index}"),
                category_id: categories[category].id,
                supplier_id: None,
                price_cents: None,
                stock_on_hand: None,
                reorder_level: None,
                weight_kg: None,
                discontinued: None,
                released_on: None,
                created_at: None,
            },
        )
        .await
        .unwrap();
    }
    let search = async |category: CategoryFilter| {
        let rows = db::products::read::search(
            pool,
            ProductFilter {
                category: Some(Box::new(category)),
                ..Default::default()
            },
            Sort::asc(ProductSortKey::Sku),
            Page::first(100),
        )
        .await
        .unwrap();
        skus(&rows)
    };

    let tools = search(CategoryFilter {
        name: vec![Text::Eq("rel-tools".to_owned())],
        ..Default::default()
    })
    .await;
    assert_eq!(tools, ["REL-0", "REL-1", "REL-4"]);
    let either = search(CategoryFilter {
        name: vec![Text::StartsWith("rel-to".to_owned())],
        ..Default::default()
    })
    .await;
    assert_eq!(either, ["REL-0", "REL-1", "REL-2", "REL-3", "REL-4"]);
}

/// Creates a tenant with the given id.
async fn create_tenant(pool: &PgPool, id: u128) -> db::tenants::models::TenantId {
    use db::tenants::models::{NewTenant, TenantId};

    db::tenants::write::create(
        pool,
        NewTenant {
            id: TenantId::from(uuid::Uuid::from_u128(id)),
            name: format!("tenant {id}"),
        },
    )
    .await
    .unwrap()
    .id
}

async fn create_post(
    pool: &PgPool,
    tenant: db::tenants::models::TenantId,
    title: &str,
) -> db::posts::models::Post {
    db::posts::write::create(
        pool,
        tenant,
        db::posts::models::NewPost {
            title: title.to_owned(),
        },
    )
    .await
    .unwrap()
}

async fn create_comment(
    pool: &PgPool,
    tenant: db::tenants::models::TenantId,
    post: &db::posts::models::Post,
    author: Option<&db::users::models::User>,
) -> db::comments::models::Comment {
    db::comments::write::create(
        pool,
        tenant,
        db::comments::models::NewComment {
            post_id: post.id,
            author_id: author.map(|author| author.id),
            body: format!("on {}", post.title),
        },
    )
    .await
    .unwrap()
}

async fn create_user(pool: &PgPool, email: &str) -> db::users::models::User {
    db::users::write::create(
        pool,
        db::users::models::NewUser {
            email: email.to_owned(),
        },
    )
    .await
    .unwrap()
}

/// The titles of the tenant's posts matching `filter`, in key order.
async fn post_titles(
    pool: &PgPool,
    tenant: db::tenants::models::TenantId,
    filter: db::posts::filter::PostFilter,
) -> Vec<String> {
    use db::posts::filter::PostSortKey;
    use db::search::{Page, Sort};

    db::posts::read::search(pool, tenant, filter, Sort::asc(PostSortKey::Id), Page::first(100))
        .await
        .unwrap()
        .into_iter()
        .map(|post| post.title)
        .collect()
}

/// Posts with at least one comment by an author: each post once, however
/// many of its comments match; an empty comment filter means "has a
/// comment".
async fn has_many_filter_matches_each_parent_once(pool: &PgPool) {
    use db::comments::filter::CommentFilter;
    use db::posts::filter::PostFilter;
    use db::search::Text;
    use db::users::filter::UserFilter;

    let tenant = create_tenant(pool, 22_101).await;
    let alice = create_user(pool, "alice@rel.example").await;
    let bob = create_user(pool, "bob@rel.example").await;
    let busy = create_post(pool, tenant, "busy").await;
    let single = create_post(pool, tenant, "single").await;
    create_post(pool, tenant, "silent").await;
    for _ in 0..3 {
        create_comment(pool, tenant, &busy, Some(&alice)).await;
    }
    create_comment(pool, tenant, &busy, Some(&bob)).await;
    create_comment(pool, tenant, &single, Some(&bob)).await;

    let by = |email: &str| PostFilter {
        comments: Some(Box::new(CommentFilter {
            author: Some(Box::new(UserFilter {
                email: vec![Text::Eq(email.to_owned())],
                ..Default::default()
            })),
            ..Default::default()
        })),
        ..Default::default()
    };
    assert_eq!(post_titles(pool, tenant, by("alice@rel.example")).await, ["busy"]);
    assert_eq!(
        post_titles(pool, tenant, by("bob@rel.example")).await,
        ["busy", "single"]
    );
    assert_eq!(
        post_titles(pool, tenant, by("nobody@rel.example")).await,
        Vec::<String>::new()
    );

    let commented = PostFilter {
        comments: Some(Box::default()),
        ..Default::default()
    };
    assert_eq!(
        post_titles(pool, tenant, commented).await,
        ["busy", "single"],
        "an empty filter means at least one comment"
    );
}

/// Soft-deleted and other tenants' related rows never satisfy a
/// relationship filter, in either direction; an empty to-one filter needs a
/// non-null, live related row.
async fn relationship_filters_skip_deleted_and_other_tenant_rows(pool: &PgPool) {
    use db::comments::filter::{CommentFilter, CommentSortKey};
    use db::posts::filter::PostFilter;
    use db::search::{Page, Sort};

    let tenant = create_tenant(pool, 22_201).await;
    let other = create_tenant(pool, 22_202).await;
    let author = create_user(pool, "carol@rel.example").await;
    let deleted_only = create_post(pool, tenant, "deleted only").await;
    let foreign_only = create_post(pool, tenant, "foreign only").await;
    let live = create_post(pool, tenant, "live").await;
    let gone = create_post(pool, tenant, "gone").await;

    let deleted = create_comment(pool, tenant, &deleted_only, Some(&author)).await;
    assert!(
        db::comments::write::delete(pool, tenant, deleted.id)
            .await
            .unwrap()
    );
    // The foreign key does not stop another tenant's comment pointing here.
    let foreign = create_comment(pool, other, &foreign_only, Some(&author)).await;
    let anonymous = create_comment(pool, tenant, &live, None).await;
    let signed = create_comment(pool, tenant, &live, Some(&author)).await;
    let orphaned = create_comment(pool, tenant, &gone, Some(&author)).await;
    assert!(db::posts::write::delete(pool, tenant, gone.id).await.unwrap());

    let commented = PostFilter {
        comments: Some(Box::default()),
        ..Default::default()
    };
    assert_eq!(post_titles(pool, tenant, commented.clone()).await, ["live"]);
    let with_deleted_posts = PostFilter {
        include_deleted: true,
        ..commented
    };
    assert_eq!(
        post_titles(pool, tenant, with_deleted_posts).await,
        ["live", "gone"],
        "include_deleted applies to the searched table only"
    );

    let comment_ids = async |tenant, filter: CommentFilter| {
        db::comments::read::search(
            pool,
            tenant,
            filter,
            Sort::asc(CommentSortKey::Id),
            Page::first(100),
        )
        .await
        .unwrap()
        .into_iter()
        .map(|comment| comment.id)
        .collect::<Vec<_>>()
    };
    let with_live_post = CommentFilter {
        post: Some(Box::default()),
        ..Default::default()
    };
    assert_eq!(
        comment_ids(tenant, with_live_post.clone()).await,
        [anonymous.id, signed.id],
        "a soft-deleted post is no parent"
    );
    assert_eq!(
        comment_ids(other, with_live_post).await,
        Vec::new(),
        "another tenant's post is no parent"
    );
    let all = comment_ids(other, CommentFilter::default()).await;
    assert_eq!(all, [foreign.id]);

    let authored = CommentFilter {
        author: Some(Box::default()),
        ..Default::default()
    };
    assert_eq!(
        comment_ids(tenant, authored).await,
        [signed.id, orphaned.id],
        "a NULL foreign key has no author"
    );
}

/// A `parent_id` self-reference filters both ways, nested; relationship
/// filters nest at most `MAX_RELATIONSHIP_DEPTH` deep.
async fn self_reference_filters_and_nesting_depth(pool: &PgPool) {
    use db::org_units::filter::{OrgUnitFilter, OrgUnitSortKey};
    use db::org_units::models::NewOrgUnit;
    use db::search::{MAX_RELATIONSHIP_DEPTH, Page, Sort, Text};

    let unit = async |parent_id, name: &str| {
        db::org_units::write::create(
            pool,
            NewOrgUnit {
                parent_id,
                name: name.to_owned(),
            },
        )
        .await
        .unwrap()
    };
    let top = unit(None, "rel-top").await;
    let middle = unit(Some(top.id), "rel-middle").await;
    unit(Some(middle.id), "rel-bottom").await;
    let names = async |filter: OrgUnitFilter| {
        db::org_units::read::search(
            pool,
            filter,
            Sort::asc(OrgUnitSortKey::Id),
            Page::first(100),
        )
        .await
        .map(|units| units.into_iter().map(|unit| unit.name).collect::<Vec<_>>())
    };
    let named = |name: &str| OrgUnitFilter {
        name: vec![Text::Eq(name.to_owned())],
        ..Default::default()
    };

    let under_top = OrgUnitFilter {
        parent: Some(Box::new(named("rel-top"))),
        ..Default::default()
    };
    assert_eq!(names(under_top).await.unwrap(), ["rel-middle"]);
    let above_bottom = OrgUnitFilter {
        children: Some(Box::new(named("rel-bottom"))),
        ..Default::default()
    };
    assert_eq!(names(above_bottom).await.unwrap(), ["rel-middle"]);

    // `hops` nested `parent` filters ending at `rel-top`.
    let ancestors = |hops: usize| {
        let mut filter = named("rel-top");
        for _ in 0..hops {
            filter = OrgUnitFilter {
                parent: Some(Box::new(filter)),
                ..Default::default()
            };
        }
        filter
    };
    assert_eq!(names(ancestors(2)).await.unwrap(), ["rel-bottom"]);
    assert_eq!(
        names(ancestors(MAX_RELATIONSHIP_DEPTH)).await.unwrap(),
        Vec::<String>::new()
    );
    let too_deep = names(ancestors(MAX_RELATIONSHIP_DEPTH + 1)).await;
    assert!(
        matches!(too_deep, Err(sqlx::Error::InvalidArgument(_))),
        "{too_deep:?}"
    );
}

/// `tests/fixture_compile.rs` renames `expense_claims.created_by_user` to
/// `author` and its inverse on `users` to `authored_expense_claims`, and opts
/// out `expense_claims.approved_by_user`, which removes both of its sides:
/// the renamed loaders work like inferred ones, and no `load_approved_by_user`
/// or `load_expense_claims_by_approved_by_user` exists to call.
async fn renamed_relationships_load_under_their_new_names(pool: &PgPool) {
    use db::expense_claims::models::NewExpenseClaim;
    use db::expense_claims::relationships::{ExpenseClaimWithAuthor, get_with_author, load_author};
    use db::users::models::NewUser;
    use db::users::relationships::load_authored_expense_claims;

    let user = async |email: &str| {
        db::users::write::create(
            pool,
            NewUser {
                email: email.to_owned(),
            },
        )
        .await
        .unwrap()
    };
    let author = user("author@example.com").await;
    let approver = user("approver@example.com").await;
    let claim = async |amount_cents| {
        db::expense_claims::write::create(
            pool,
            NewExpenseClaim {
                created_by_user_id: author.id,
                approved_by_user_id: Some(approver.id),
                amount_cents,
            },
        )
        .await
        .unwrap()
    };
    let first = claim(100).await;
    let second = claim(200).await;

    let authors = load_author(pool, &[first.clone(), second.clone()])
        .await
        .unwrap();
    assert_eq!(authors.len(), 1);
    assert_eq!(authors[&author.id], author);

    let authored = load_authored_expense_claims(pool, &[author.clone(), approver.clone()])
        .await
        .unwrap();
    assert_eq!(authored[&author.id], vec![first.clone(), second]);
    assert_eq!(authored[&approver.id], Vec::new());

    // The joined read takes the new name too; the opted-out
    // `approved_by_user` has none (`get_with_approved_by_user` does not
    // exist, see `tests/relationships.rs`).
    let ExpenseClaimWithAuthor {
        expense_claim,
        author: read_author,
    } = get_with_author(pool, first.id).await.unwrap().unwrap();
    assert_eq!(expense_claim, first);
    assert_eq!(read_author, author);
}

/// `get_with_category` reads a product and its category in one statement.
/// Both tables have `id` and `name`; each row gets its own values.
async fn joined_read_fetches_a_row_with_its_parent_in_one_query(pool: &PgPool) {
    use db::categories::models::NewCategory;
    use db::products::models::{NewProduct, ProductId};
    use db::products::relationships::{ProductWithCategory, get_with_category};

    let category = db::categories::write::create(
        pool,
        NewCategory {
            name: "joined category".to_owned(),
            description: Some("shares column names".to_owned()),
        },
    )
    .await
    .unwrap();
    let product = db::products::write::create(
        pool,
        NewProduct {
            id: ProductId::from(uuid::Uuid::from_u128(21_000)),
            sku: "J-1".to_owned(),
            name: "joined product".to_owned(),
            category_id: category.id,
            supplier_id: None,
            price_cents: Some(1_250),
            stock_on_hand: None,
            reorder_level: None,
            weight_kg: None,
            discontinued: None,
            released_on: None,
            created_at: None,
        },
    )
    .await
    .unwrap();

    let mut tx = pool.begin().await.unwrap();
    let products_before = scans_in_transaction(&mut tx, "products").await;
    let categories_before = scans_in_transaction(&mut tx, "categories").await;
    let joined = get_with_category(&mut *tx, product.id)
        .await
        .unwrap()
        .expect("the product exists");
    let products_after = scans_in_transaction(&mut tx, "products").await;
    let categories_after = scans_in_transaction(&mut tx, "categories").await;
    tx.rollback().await.unwrap();
    assert_eq!(products_after - products_before, 1, "one statement");
    assert_eq!(categories_after - categories_before, 1, "one statement");

    // With `derive = ["serde"]`, the joined struct goes to JSON and back too.
    let json = serde_json::to_value(&joined).unwrap();
    assert_eq!(json["category"]["name"], category.name.as_str());
    assert_eq!(
        serde_json::from_value::<ProductWithCategory>(json).unwrap(),
        joined
    );

    // The category is not optional: the foreign key is NOT NULL and
    // categories are never hidden.
    let ProductWithCategory {
        product: read_product,
        category: read_category,
    } = joined;
    assert_eq!(read_product, product);
    assert_eq!(read_category, category);
    assert_ne!(read_product.name, read_category.name);

    let missing = ProductId::from(uuid::Uuid::from_u128(21_999));
    assert_eq!(get_with_category(pool, missing).await.unwrap(), None);
}

/// A NULL foreign key reads the row with `None` as the related row; a set
/// one reads its parent, and a `parent_id` self-reference joins the table
/// to itself. A has-one reads the parent with its child, or with `None`.
async fn joined_read_of_a_nullable_foreign_key_without_a_parent(pool: &PgPool) {
    use db::org_units::models::NewOrgUnit;
    use db::org_units::relationships::get_with_parent;
    use db::user_profiles::models::NewUserProfile;
    use db::users::models::NewUser;
    use db::users::relationships::get_with_user_profile;

    let unit = async |parent_id, name: &str| {
        db::org_units::write::create(
            pool,
            NewOrgUnit {
                parent_id,
                name: name.to_owned(),
            },
        )
        .await
        .unwrap()
    };
    let root = unit(None, "joined root").await;
    let child = unit(Some(root.id), "joined child").await;

    let orphan = get_with_parent(pool, root.id).await.unwrap().unwrap();
    assert_eq!(orphan.org_unit, root);
    assert_eq!(orphan.parent, None);
    let joined = get_with_parent(pool, child.id).await.unwrap().unwrap();
    assert_eq!(joined.org_unit, child);
    assert_eq!(joined.parent, Some(root));

    let user = db::users::write::create(
        pool,
        NewUser {
            email: "joined@example.com".to_owned(),
        },
    )
    .await
    .unwrap();
    let alone = get_with_user_profile(pool, user.id).await.unwrap().unwrap();
    assert_eq!(alone.user, user);
    assert_eq!(alone.user_profile, None);
    let profile = db::user_profiles::write::create(
        pool,
        NewUserProfile {
            user_id: user.id,
            bio: "joined".to_owned(),
        },
    )
    .await
    .unwrap();
    let profiled = get_with_user_profile(pool, user.id).await.unwrap().unwrap();
    assert_eq!(profiled.user_profile, Some(profile));
}

/// Comments and posts are both tenant-scoped and soft-deleted. A deleted or
/// other-tenant post reads as an absent parent and keeps the comment; a
/// deleted or other-tenant comment reads as `None`.
async fn joined_read_respects_soft_delete_and_tenant_on_both_sides(pool: &PgPool) {
    use db::comments::models::NewComment;
    use db::comments::relationships::get_with_post;
    use db::posts::models::NewPost;
    use db::tenants::models::{NewTenant, TenantId};

    let tenant = async |id: u128| {
        db::tenants::write::create(
            pool,
            NewTenant {
                id: TenantId::from(uuid::Uuid::from_u128(id)),
                name: format!("tenant {id}"),
            },
        )
        .await
        .unwrap()
        .id
    };
    let tenant_a = tenant(21_101).await;
    let tenant_b = tenant(21_102).await;
    let post = async |tenant, title: &str| {
        db::posts::write::create(
            pool,
            tenant,
            NewPost {
                title: title.to_owned(),
            },
        )
        .await
        .unwrap()
    };
    let comment = async |tenant, post_id, body: &str| {
        db::comments::write::create(
            pool,
            tenant,
            NewComment {
                post_id,
                author_id: None,
                body: body.to_owned(),
            },
        )
        .await
        .unwrap()
    };
    let live_post = post(tenant_a, "live").await;
    let doomed_post = post(tenant_a, "doomed").await;
    let foreign_post = post(tenant_b, "foreign").await;
    let on_live = comment(tenant_a, live_post.id, "on live").await;
    let on_doomed = comment(tenant_a, doomed_post.id, "on doomed").await;
    // The foreign key does not stop a row from pointing at another tenant's.
    let on_foreign = comment(tenant_a, foreign_post.id, "on foreign").await;

    let joined = get_with_post(pool, tenant_a, on_live.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(joined.comment, on_live);
    assert_eq!(joined.post, Some(live_post));

    assert!(
        db::posts::write::delete(pool, tenant_a, doomed_post.id)
            .await
            .unwrap()
    );
    let orphaned = get_with_post(pool, tenant_a, on_doomed.id)
        .await
        .unwrap()
        .expect("a deleted parent does not hide the child");
    assert_eq!(orphaned.comment, on_doomed);
    assert_eq!(orphaned.post, None);

    let crossed = get_with_post(pool, tenant_a, on_foreign.id)
        .await
        .unwrap()
        .expect("the comment is the tenant's");
    assert_eq!(crossed.post, None, "the post is another tenant's");

    assert_eq!(
        get_with_post(pool, tenant_b, on_live.id).await.unwrap(),
        None,
        "the comment is another tenant's"
    );
    assert!(
        db::comments::write::delete(pool, tenant_a, on_live.id)
            .await
            .unwrap()
    );
    assert_eq!(
        get_with_post(pool, tenant_a, on_live.id).await.unwrap(),
        None,
        "a deleted child reads as missing"
    );
}

/// Relationship filters use the configured names: `author` and
/// `authored_expense_claims` filter like inferred relationships, and the
/// opted-out `approved_by_user` has no filter field (only its column's).
async fn renamed_relationships_filter_under_their_new_names(pool: &PgPool) {
    use db::expense_claims::filter::{ExpenseClaimFilter, ExpenseClaimSortKey};
    use db::expense_claims::models::NewExpenseClaim;
    use db::search::{Ordered, Page, Sort, Text};
    use db::users::filter::{UserFilter, UserSortKey};

    let writer = create_user(pool, "writer@claims.example").await;
    let reviewer = create_user(pool, "reviewer@claims.example").await;
    let claim = db::expense_claims::write::create(
        pool,
        NewExpenseClaim {
            created_by_user_id: writer.id,
            approved_by_user_id: Some(reviewer.id),
            amount_cents: 22_500,
        },
    )
    .await
    .unwrap();

    let claims = db::expense_claims::read::search(
        pool,
        ExpenseClaimFilter {
            author: Some(Box::new(UserFilter {
                email: vec![Text::Eq("writer@claims.example".to_owned())],
                ..Default::default()
            })),
            // The opted-out relationship leaves only its column's conditions.
            approved_by_user_id: vec![Ordered::Eq(reviewer.id)],
            ..Default::default()
        },
        Sort::asc(ExpenseClaimSortKey::Id),
        Page::first(10),
    )
    .await
    .unwrap();
    assert_eq!(claims, vec![claim]);

    let authors = db::users::read::search(
        pool,
        UserFilter {
            authored_expense_claims: Some(Box::new(ExpenseClaimFilter {
                amount_cents: vec![Ordered::Eq(22_500)],
                ..Default::default()
            })),
            ..Default::default()
        },
        Sort::asc(UserSortKey::Id),
        Page::first(10),
    )
    .await
    .unwrap();
    assert_eq!(authors, vec![writer], "the reviewer only approved it");
}

/// Two tenants, two roles in the first, and three permissions shared by all,
/// for the many-to-many scenarios.
async fn seed_roles(
    pool: &PgPool,
    first_tenant: u128,
) -> (
    [db::tenants::models::TenantId; 2],
    Vec<db::roles::models::Role>,
    Vec<db::permissions::models::Permission>,
) {
    use db::permissions::models::{NewPermission, PermissionId};
    use db::roles::models::NewRole;
    use db::tenants::models::{NewTenant, TenantId};

    let mut tenants = Vec::new();
    for id in [first_tenant, first_tenant + 1] {
        let tenant = db::tenants::write::create(
            pool,
            NewTenant {
                id: TenantId::from(uuid::Uuid::from_u128(id)),
                name: format!("tenant {id}"),
            },
        )
        .await
        .unwrap();
        tenants.push(tenant.id);
    }
    let mut roles = Vec::new();
    for name in ["admin", "clerk"] {
        let role = db::roles::write::create(
            pool,
            tenants[0],
            NewRole {
                name: name.to_owned(),
            },
        )
        .await
        .unwrap();
        roles.push(role);
    }
    let mut permissions = Vec::new();
    for code in ["read", "refund", "write"] {
        let code = format!("{code}-{first_tenant}");
        let permission = db::permissions::write::create(
            pool,
            NewPermission {
                code: PermissionId::from(code.clone()),
                description: format!("may {code}"),
            },
        )
        .await
        .unwrap();
        permissions.push(permission);
    }
    ([tenants[0], tenants[1]], roles, permissions)
}

/// `role_permissions` pairs roles with permissions: `attach` is idempotent,
/// `detach` removes one pair, `sync` replaces a role's set (an empty slice
/// removes all), and both loaders see the result, inside the tenant.
async fn many_to_many_attach_detach_and_sync(pool: &PgPool) {
    use db::permissions::relationships::load_roles;
    use db::role_permissions::write::{attach, detach, sync};
    use db::roles::relationships::load_permissions;

    let ([tenant, other_tenant], roles, permissions) = seed_roles(pool, 23_001).await;
    let [admin, clerk] = [&roles[0], &roles[1]];
    let [read, refund, write] = [&permissions[0], &permissions[1], &permissions[2]];

    assert!(attach(pool, tenant, admin.id, &read.code).await.unwrap());
    assert!(
        !attach(pool, tenant, admin.id, &read.code).await.unwrap(),
        "already attached"
    );
    assert!(attach(pool, tenant, admin.id, &write.code).await.unwrap());
    assert!(attach(pool, tenant, clerk.id, &read.code).await.unwrap());
    assert_eq!(
        db::role_permissions::read::count(pool, tenant).await.unwrap(),
        3,
        "attaching twice keeps one row"
    );
    // Another tenant's pairing of the same rows stays in that tenant.
    assert!(
        attach(pool, other_tenant, clerk.id, &refund.code)
            .await
            .unwrap()
    );

    let loaded = load_permissions(pool, tenant, &roles).await.unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[&admin.id], vec![read.clone(), write.clone()]);
    assert_eq!(loaded[&clerk.id], vec![read.clone()]);
    let holders = load_roles(pool, tenant, &permissions).await.unwrap();
    assert_eq!(holders[&read.code], vec![admin.clone(), clerk.clone()]);
    assert_eq!(holders[&refund.code], Vec::new());
    assert_eq!(holders[&write.code], vec![admin.clone()]);
    let elsewhere = load_permissions(pool, other_tenant, &roles).await.unwrap();
    assert_eq!(elsewhere[&clerk.id], vec![refund.clone()]);
    assert_eq!(elsewhere[&admin.id], Vec::new());

    assert!(detach(pool, tenant, admin.id, &write.code).await.unwrap());
    assert!(
        !detach(pool, tenant, admin.id, &write.code).await.unwrap(),
        "already detached"
    );
    let loaded = load_permissions(pool, tenant, &roles).await.unwrap();
    assert_eq!(loaded[&admin.id], vec![read.clone()]);

    // `sync` runs in the caller's transaction: rolled back, nothing changes.
    let mut tx = pool.begin().await.unwrap();
    let codes = [refund.code.clone(), write.code.clone(), write.code.clone()];
    sync(&mut *tx, tenant, admin.id, &codes).await.unwrap();
    let inside = load_permissions(&mut *tx, tenant, &roles).await.unwrap();
    assert_eq!(inside[&admin.id], vec![refund.clone(), write.clone()]);
    tx.rollback().await.unwrap();
    let loaded = load_permissions(pool, tenant, &roles).await.unwrap();
    assert_eq!(loaded[&admin.id], vec![read.clone()]);

    sync(pool, tenant, admin.id, &codes).await.unwrap();
    let loaded = load_permissions(pool, tenant, &roles).await.unwrap();
    assert_eq!(loaded[&admin.id], vec![refund.clone(), write.clone()]);
    assert_eq!(loaded[&clerk.id], vec![read.clone()], "other roles keep theirs");

    sync(pool, tenant, admin.id, &[]).await.unwrap();
    let loaded = load_permissions(pool, tenant, &roles).await.unwrap();
    assert_eq!(loaded[&admin.id], Vec::new(), "an empty slice removes all");
    assert_eq!(loaded[&clerk.id], vec![read.clone()]);
    let elsewhere = load_permissions(pool, other_tenant, &roles).await.unwrap();
    assert_eq!(elsewhere[&clerk.id], vec![refund.clone()]);
}

/// On the soft-delete join table, `detach` soft-deletes the pairing and
/// `attach` (or `sync`) restores that same row instead of inserting another.
async fn soft_delete_join_rows_are_restored_not_duplicated(pool: &PgPool) {
    use db::permissions::models::PermissionId;
    use db::role_permissions::write::{attach, detach, sync};
    use db::roles::relationships::load_permissions;

    let ([tenant, _], roles, permissions) = seed_roles(pool, 23_101).await;
    let role = &roles[0];
    let [read, refund, write] = [&permissions[0], &permissions[1], &permissions[2]];
    let pairing = async |code: &PermissionId| {
        let rows = db::role_permissions::read::list(pool, tenant).await.unwrap();
        rows.into_iter()
            .filter(|row| row.role_id == role.id && row.permission_code == *code)
            .collect::<Vec<_>>()
    };

    assert!(attach(pool, tenant, role.id, &read.code).await.unwrap());
    let original = pairing(&read.code).await;
    assert_eq!(original.len(), 1);
    assert!(detach(pool, tenant, role.id, &read.code).await.unwrap());
    assert_eq!(pairing(&read.code).await, Vec::new(), "soft-deleted");
    let loaded = load_permissions(pool, tenant, &roles).await.unwrap();
    assert_eq!(loaded[&role.id], Vec::new());

    assert!(attach(pool, tenant, role.id, &read.code).await.unwrap());
    let restored = pairing(&read.code).await;
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].id, original[0].id, "the original row is restored");
    assert_eq!(restored[0].granted_at, original[0].granted_at);
    assert_eq!(restored[0].deleted_at, None);

    // `sync` soft-deletes what it drops and restores what it brings back.
    sync(pool, tenant, role.id, std::slice::from_ref(&write.code))
        .await
        .unwrap();
    let written = pairing(&write.code).await;
    assert_eq!(pairing(&read.code).await, Vec::new());
    let codes = [read.code.clone(), refund.code.clone(), write.code.clone()];
    sync(pool, tenant, role.id, &codes).await.unwrap();
    assert_eq!(pairing(&read.code).await[0].id, original[0].id);
    assert_eq!(pairing(&write.code).await, written);
    assert_eq!(pairing(&refund.code).await.len(), 1);
    let loaded = load_permissions(pool, tenant, &roles).await.unwrap();
    assert_eq!(
        loaded[&role.id],
        vec![read.clone(), refund.clone(), write.clone()]
    );
}

/// `group_members` has no soft-delete column, so detaching and syncing
/// delete rows; its key is the pair itself.
async fn plain_join_table_deletes_detached_pairs(pool: &PgPool) {
    use db::group_members::write::{attach, detach, sync};
    use db::member_groups::models::NewMemberGroup;
    use db::member_groups::relationships::load_users;
    use db::users::models::NewUser;
    use db::users::relationships::load_member_groups;

    let group = db::member_groups::write::create(
        pool,
        NewMemberGroup {
            name: "night shift".to_owned(),
        },
    )
    .await
    .unwrap();
    let mut users = Vec::new();
    for email in ["ann@groups.example", "bo@groups.example", "cy@groups.example"] {
        let user = db::users::write::create(
            pool,
            NewUser {
                email: email.to_owned(),
            },
        )
        .await
        .unwrap();
        users.push(user);
    }
    let groups = [group.clone()];

    assert!(attach(pool, group.id, users[0].id).await.unwrap());
    assert!(!attach(pool, group.id, users[0].id).await.unwrap());
    assert!(attach(pool, group.id, users[1].id).await.unwrap());
    let members = load_users(pool, &groups).await.unwrap();
    assert_eq!(members[&group.id], vec![users[0].clone(), users[1].clone()]);
    let memberships = load_member_groups(pool, &users).await.unwrap();
    assert_eq!(memberships[&users[0].id], vec![group.clone()]);
    assert_eq!(memberships[&users[2].id], Vec::new());

    assert!(detach(pool, group.id, users[0].id).await.unwrap());
    assert!(!detach(pool, group.id, users[0].id).await.unwrap());
    sync(pool, group.id, &[users[1].id, users[2].id])
        .await
        .unwrap();
    let members = load_users(pool, &groups).await.unwrap();
    assert_eq!(members[&group.id], vec![users[1].clone(), users[2].clone()]);
    sync(pool, group.id, &[]).await.unwrap();
    let members = load_users(pool, &groups).await.unwrap();
    assert_eq!(members[&group.id], Vec::new());
}

/// A many-to-many filter field matches rows paired, through a live row of
/// the join table in the same tenant, with a related row matching the filter.
async fn many_to_many_filters_go_through_the_join_table(pool: &PgPool) {
    use db::member_groups::filter::{MemberGroupFilter, MemberGroupSortKey};
    use db::member_groups::models::NewMemberGroup;
    use db::permissions::filter::PermissionFilter;
    use db::roles::filter::{RoleFilter, RoleSortKey};
    use db::search::{Page, Sort, Text};
    use db::users::filter::{UserFilter, UserSortKey};

    let ([tenant, other_tenant], roles, permissions) = seed_roles(pool, 23_201).await;
    let [admin, clerk] = [&roles[0], &roles[1]];
    let [read, refund, _] = [&permissions[0], &permissions[1], &permissions[2]];
    let attach = db::role_permissions::write::attach;
    assert!(attach(pool, tenant, admin.id, &refund.code).await.unwrap());
    assert!(attach(pool, tenant, clerk.id, &read.code).await.unwrap());
    // Paired in another tenant only, or no longer paired: neither matches.
    assert!(attach(pool, other_tenant, clerk.id, &refund.code).await.unwrap());
    assert!(attach(pool, tenant, clerk.id, &refund.code).await.unwrap());
    assert!(
        db::role_permissions::write::detach(pool, tenant, clerk.id, &refund.code)
            .await
            .unwrap()
    );

    let refunders = db::roles::read::search(
        pool,
        tenant,
        RoleFilter {
            permissions: Some(Box::new(PermissionFilter {
                code: vec![Text::Eq(refund.code.to_string())],
                ..Default::default()
            })),
            ..Default::default()
        },
        Sort::asc(RoleSortKey::Id),
        Page::first(10),
    )
    .await
    .unwrap();
    assert_eq!(refunders, vec![admin.clone()]);

    let group = db::member_groups::write::create(
        pool,
        NewMemberGroup {
            name: "filtered group".to_owned(),
        },
    )
    .await
    .unwrap();
    let member = create_user(pool, "member@filters.example").await;
    let outsider = create_user(pool, "outsider@filters.example").await;
    assert!(
        db::group_members::write::attach(pool, group.id, member.id)
            .await
            .unwrap()
    );
    let members = db::users::read::search(
        pool,
        UserFilter {
            member_groups: Some(Box::new(MemberGroupFilter {
                name: vec![Text::Eq("filtered group".to_owned())],
                ..Default::default()
            })),
            ..Default::default()
        },
        Sort::asc(UserSortKey::Id),
        Page::first(10),
    )
    .await
    .unwrap();
    assert_eq!(members, vec![member.clone()]);
    let groups = db::member_groups::read::search(
        pool,
        MemberGroupFilter {
            users: Some(Box::new(UserFilter {
                email: vec![Text::Eq(outsider.email.clone())],
                ..Default::default()
            })),
            ..Default::default()
        },
        Sort::asc(MemberGroupSortKey::Id),
        Page::first(10),
    )
    .await
    .unwrap();
    assert_eq!(groups, Vec::new());
}
