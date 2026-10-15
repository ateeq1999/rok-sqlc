//! Executes the generated CRUD code against a real PostgreSQL database.
//!
//! Runs only when `ROK_SQLC_TEST_DATABASE_URL` is set. The database is reset
//! (its `public` and `app` schemas are dropped), so point it at a throwaway
//! database, never one holding data you care about.

use rok_sqlc_fixture_app::db;
use rust_decimal::Decimal;
use sqlx::PgPool;

use db::patch_targets::models::{NewPatchTarget, PatchTarget, UpdatePatchTarget};

async fn fresh_database() -> Option<PgPool> {
    let Ok(url) = std::env::var("ROK_SQLC_TEST_DATABASE_URL") else {
        eprintln!("skipping: ROK_SQLC_TEST_DATABASE_URL is not set");
        return None;
    };
    let pool = PgPool::connect(&url).await.expect("connect to test database");
    sqlx::raw_sql(
        "DROP SCHEMA IF EXISTS app CASCADE; DROP SCHEMA IF EXISTS public CASCADE; CREATE SCHEMA public;",
    )
    .execute(&pool)
    .await
    .expect("reset test database");
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
}

/// Seeds five products in a fresh category, priced 100..=500, and returns
/// the category id.
async fn seed_search_products(pool: &PgPool) -> i64 {
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
                id: uuid::Uuid::from_u128(506 - n as u128),
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

async fn keyset_pagination_follows_the_primary_key(pool: &PgPool, category: i64) {
    use db::feature_line_items::filter::FeatureLineItemFilter;
    use db::feature_line_items::models::NewFeatureLineItem;
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
        Some((id(1), id(1))),
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

async fn sorting_is_allowlisted_and_stable(pool: &PgPool, category: i64) {
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

async fn null_checks_and_equality_filters(pool: &PgPool, category: i64) {
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
            uuid::Uuid::from_u128(501),
            uuid::Uuid::from_u128(503),
        ])],
        supplier_id_is_null: Some(true),
        ..Default::default()
    })
    .await;
    assert_eq!(by_id, ["S-1", "S-2", "S-4"], "ids 501 and 503 are S-5 and S-3");

    let picked = skus_where(ProductFilter {
        id: vec![Equality::In(vec![
            uuid::Uuid::from_u128(501),
            uuid::Uuid::from_u128(502),
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

async fn text_filters_escape_like_wildcards(pool: &PgPool, category: i64) {
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
            id: uuid::Uuid::from_u128(900),
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
            category_id: category.id,
            supplier_id: Some(supplier.id),
            id_2: uuid::Uuid::from_u128(102),
            sku_2: "Q-2".to_owned(),
            name_2: "Query anvil".to_owned(),
            category_id_2: category.id,
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
    let low_stock: Vec<String> = queries::list_products_by_category(pool, category.id, 10)
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
    assert_eq!(stocked[0].product_count, 1, "only the hammer has stock >= 1");
    assert_eq!(stocked[0].total_stock, Some(50));
    assert!(
        queries::stocked_categories(pool, 51, category.id)
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

    let missing_id = i64::MAX;
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
                id: uuid::Uuid::from_u128(id),
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
        .map(|row| row.id.as_u128())
        .collect();
    assert_eq!(all, [1, 2, 3, 4, 5]);

    let mut paged = Vec::new();
    for page in 0..3 {
        let rows = read::list_page(pool, 2, page * 2).await.unwrap();
        paged.extend(rows.iter().map(|row| row.id.as_u128()));
    }
    assert_eq!(paged, all, "pages neither repeat nor skip rows");
    assert_eq!(read::count(pool).await.unwrap(), 5);

    let first = read::get_by_id(pool, uuid::Uuid::from_u128(3)).await.unwrap().unwrap();
    assert_eq!(first.price_cents, 1999);
    assert_eq!(first.weight_kg, Some(1.5));
}

async fn composite_keys_arrays_and_numeric(pool: &PgPool) {
    use db::feature_line_items::models::{NewFeatureLineItem, UpdateFeatureLineItem};
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
    let updated = write::update(pool, order_id, product_id, patch)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.weekdays, None);
    assert_eq!(updated.unit_price, Decimal::new(2500, 2));
    assert_eq!(updated.labels, ["a", "b"]);

    assert!(read::exists_by_id(pool, order_id, product_id).await.unwrap());
    assert!(!read::exists_by_id(pool, product_id, order_id).await.unwrap(), "key order matters");
    assert!(write::delete(pool, order_id, product_id).await.unwrap());
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
