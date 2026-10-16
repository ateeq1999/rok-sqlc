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
    let pool = PgPool::connect(&url).await.expect("connect to test database");
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
    soft_deleted_rows_are_hidden_restored_and_removed(&pool).await;
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
    assert_eq!(found, vec![product.clone()], "a category's id filters its products");

    let fetched = db::products::read::get_by_id(pool, product_id).await.unwrap();
    assert_eq!(fetched, Some(product));
    let parent = db::categories::read::get_by_id(pool, category.id).await.unwrap();
    assert_eq!(parent.map(|row| row.name), Some("typed".to_owned()));

    // `Copy`: the id is used again after being passed by value above.
    let raw: i64 = category.id.into();
    assert_eq!(CategoryId::from(raw), category.id);
    assert_eq!(category.id.to_string(), raw.to_string());
    assert_eq!(product_id.to_string(), "00000000-0000-0000-0000-000000001b59");
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
        let page = search_after(pool, in_category(), after, Limit::new(2)).await.unwrap();
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
    assert_eq!(last.offset(), i64::MAX, "next() saturates instead of overflowing");
    let one = search_after(pool, in_category(), None, Limit::new(0)).await.unwrap();
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
    assert_eq!(by_id, ["S-1", "S-2", "S-4"], "ids 501 and 503 are S-5 and S-3");

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
    use db::queries::inventory::{self as queries, CreateProductsParams};

    let category = queries::create_category(pool, "query-cat".to_owned(), None)
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
            id: uuid::Uuid::from_u128(101),
            sku: "Q-1".to_owned(),
            name: "Query hammer".to_owned(),
            category_id: category.id.into(),
            supplier_id: Some(supplier.id.into()),
            id_2: uuid::Uuid::from_u128(102),
            sku_2: "Q-2".to_owned(),
            name_2: "Query anvil".to_owned(),
            category_id_2: category.id.into(),
            supplier_id_2: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(inserted, 2, "both VALUES rows bind to the right placeholders");

    let hammer = queries::get_product_by_sku(pool, "Q-1".to_owned())
        .await
        .unwrap()
        .expect("Q-1 exists");
    assert_eq!(hammer.id, uuid::Uuid::from_u128(101));
    assert_eq!(hammer.name, "Query hammer");
    assert_eq!(
        queries::get_product_by_sku(pool, "missing".to_owned())
            .await
            .unwrap(),
        None
    );

    queries::update_product_stock(pool, uuid::Uuid::from_u128(101), 50)
        .await
        .unwrap();
    let low_stock: Vec<String> = queries::list_products_by_category(pool, category.id.into(), 10)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.sku)
        .collect();
    assert_eq!(low_stock, ["Q-2"], "only the anvil has stock <= 10");

    let joined = queries::list_products_with_category(
        pool,
        vec!["Q-1".to_owned(), "Q-2".to_owned()],
    )
    .await
    .unwrap();
    assert_eq!(joined.len(), 2);
    assert_eq!(joined[0].category_name, "query-cat");
    assert_eq!(joined[0].supplier_name.as_deref(), Some("Acme"));
    assert_eq!(joined[1].supplier_name, None, "LEFT JOIN without a supplier");

    assert_eq!(
        queries::count_products_in_category(pool, category.id.into())
            .await
            .unwrap(),
        2
    );
    let stock = queries::category_stock(pool).await.unwrap();
    let ours = stock
        .iter()
        .find(|row| row.category_id == i64::from(category.id))
        .expect("category has stock");
    assert_eq!(ours.total_stock, 50);

    let stocked = queries::stocked_categories(pool, 1, category.id.into())
        .await
        .unwrap();
    assert_eq!(stocked.len(), 1, "the CTE pre-aggregates per category");
    assert_eq!(stocked[0].id, i64::from(category.id));
    assert_eq!(stocked[0].category_name, "query-cat");
    assert_eq!(stocked[0].product_count, 1, "only the hammer has stock >= 1");
    assert_eq!(stocked[0].total_stock, Some(50));
    assert!(
        queries::stocked_categories(pool, 51, category.id.into())
            .await
            .unwrap()
            .is_empty(),
        "the CTE placeholder filters before aggregation"
    );

    let found = queries::search_products(pool, "%ANVIL%".to_owned())
        .await
        .unwrap();
    assert_eq!(found, ["Q-2"]);

    assert_eq!(
        queries::delete_product_by_sku(pool, "Q-2".to_owned())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        queries::delete_product_by_sku(pool, "Q-2".to_owned())
            .await
            .unwrap(),
        0
    );
}

/// Each query file is its own module, so both define `search_products`.
/// Runs after `annotated_queries`, which leaves product Q-1 supplied by Acme.
async fn query_modules_per_file(pool: &PgPool) {
    use db::queries::{inventory, suppliers};

    let globex = suppliers::create_supplier(pool, uuid::Uuid::from_u128(901), "Globex".to_owned())
        .await
        .unwrap();
    assert_eq!(globex.name, "Globex");
    assert!(globex.is_active, "the column default applies");

    assert_eq!(
        suppliers::search_products(pool, "Acme".to_owned())
            .await
            .unwrap(),
        ["Q-1"]
    );
    assert!(
        suppliers::search_products(pool, "Globex".to_owned())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        inventory::search_products(pool, "%hammer%".to_owned())
            .await
            .unwrap(),
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
    assert_eq!(defaults.tag.as_deref(), Some("none"), "omitted tag uses DEFAULT 'none'");
    assert_eq!(defaults.note, None);
    assert_eq!(defaults.doubled, Some(0), "generated column is computed by the database");

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
    assert_eq!(explicit.tag, None, "Some(None) inserts NULL instead of the default");
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
    assert!(write::delete(pool, row.id).await.unwrap(), "existing row is deleted");
    assert!(!write::delete(pool, row.id).await.unwrap(), "second delete finds nothing");
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
    assert_eq!(row.mixed_case, 1, "quoted defaulted column applies its default");
    assert_eq!(row.type_.as_deref(), Some("kind"));
    assert_eq!(row._1st, Some(true));

    let patch = UpdateReservedWord {
        type_: Some(None),
        user: Some(Some("alice".to_owned())),
        mixed_case: Some(9),
        ..Default::default()
    };
    let updated = write::update(pool, row.select, patch).await.unwrap().unwrap();
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
        assert_eq!(product.reorder_level, 10, "defaulted SMALLINT applies DEFAULT 10");
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

    let first = read::get_by_id(pool, uuid::Uuid::from_u128(3).into()).await.unwrap().unwrap();
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
    assert!(!read::exists_by_id(pool, swapped).await.unwrap(), "key parts are not interchangeable");
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
    assert_eq!(very_high, [archived.clone()]);

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
    assert_eq!(found[0].id, i64::from(routine.id));
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
    assert_eq!(defaulted.status, ShipmentStatus::Pending, "the default applies");
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
    assert_eq!(found, [delivered.clone()]);

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
    assert_eq!(db::products::read::get_by_sku(pool, "U-3").await.unwrap(), None);

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
    assert_eq!(globex_hq.map(|row| row.name), Some("Globex head office".to_owned()));
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
