//! Search: filters, keyset pagination, and sorting.

use super::*;

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

pub(super) async fn search_filters_and_pages(pool: &PgPool) {
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
    filters_round_trip_through_json(pool, category).await;
}

/// With `derive = ["serde"]`, a filter, its sort, and its conditions
/// (including a relationship filter) go to JSON and back, and the filter
/// read back finds the same rows.
async fn filters_round_trip_through_json(pool: &PgPool, category: CategoryId) {
    use db::categories::filter::CategoryFilter;
    use db::products::filter::{ProductFilter, ProductSortKey};
    use db::products::read::search;
    use db::search::{Equality, Ordered, Page, Sort, Text};

    let filter = ProductFilter {
        category_id: vec![Ordered::Eq(category)],
        price_cents: vec![Ordered::Between(50, 350), Ordered::Ne(200)],
        name: vec![Text::Contains("l".to_owned())],
        discontinued: vec![Equality::Eq(false)],
        weight_kg_is_null: Some(false),
        category: Some(Box::new(CategoryFilter {
            name: vec![Text::Eq("search".to_owned())],
            ..Default::default()
        })),
        ..Default::default()
    };
    let sort = Sort::desc(ProductSortKey::PriceCents);
    let expected = search(pool, filter.clone(), sort, Page::first(10))
        .await
        .unwrap();
    assert_eq!(skus(&expected), ["S-3", "S-1"]);

    let json = serde_json::to_string(&(&filter, &sort)).unwrap();
    let (decoded, decoded_sort): (ProductFilter, Sort<ProductSortKey>) =
        serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, filter);
    assert_eq!(decoded_sort, sort);
    let found = search(pool, decoded, decoded_sort, Page::first(10))
        .await
        .unwrap();
    assert_eq!(found, expected);

    // Variants are snake_case in JSON.
    assert_eq!(
        serde_json::to_string(&sort).unwrap(),
        r#"{"key":"price_cents","direction":"desc"}"#
    );

    // Fields left out of the JSON keep their defaults.
    let sparse: ProductFilter =
        serde_json::from_str(r#"{"price_cents": [{"ge": 400}]}"#).unwrap();
    assert_eq!(
        sparse,
        ProductFilter {
            price_cents: vec![Ordered::Ge(400)],
            ..Default::default()
        }
    );
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
