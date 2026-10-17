//! Keys: typed ids, UUID, composite, text, and unique keys, and foreign keys.

use super::*;

/// A category's typed id goes straight into a product's foreign key field
/// and into a product filter; typed ids hash, order, display, and convert.
/// An enum primary key gets a typed id wrapping the enum; the foreign key
/// into it, lookups, and the keyset cursor take that id.
pub(super) async fn enum_keys_get_a_typed_id(pool: &PgPool) {
    use db::priority_escalations::models::NewPriorityEscalation;
    use db::priority_notes::filter::PriorityNoteFilter;
    use db::priority_notes::models::{NewPriorityNote, PriorityNoteId};
    use db::search::Limit;
    use db::types::TicketPriority;

    for (priority, note) in [(TicketPriority::Low, "later"), (TicketPriority::High, "now")] {
        db::priority_notes::write::create(
            pool,
            NewPriorityNote {
                priority: PriorityNoteId::from(priority),
                note: note.to_owned(),
            },
        )
        .await
        .unwrap();
    }
    let escalation = db::priority_escalations::write::create(
        pool,
        NewPriorityEscalation {
            priority: PriorityNoteId(TicketPriority::High),
        },
    )
    .await
    .unwrap();
    let note: PriorityNoteId = escalation.priority;

    let found = db::priority_notes::read::get_by_id(pool, note)
        .await
        .unwrap()
        .expect("the escalated note exists");
    assert_eq!(found.note, "now");
    assert_eq!(TicketPriority::from(found.priority), TicketPriority::High);

    let after = db::priority_notes::read::search_after(
        pool,
        PriorityNoteFilter::default(),
        Some(PriorityNoteId(TicketPriority::Low)),
        Limit::new(10),
    )
    .await
    .unwrap();
    let keys: Vec<PriorityNoteId> = after.iter().map(|row| row.priority).collect();
    assert_eq!(keys, [PriorityNoteId(TicketPriority::High)]);
}

pub(super) async fn typed_ids_flow_from_rows_to_foreign_keys(pool: &PgPool) {
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
pub(super) async fn foreign_keys_resolve_to_the_table_in_their_schema(pool: &PgPool) {
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

pub(super) async fn uuid_keys_listing_and_pagination(pool: &PgPool) {
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

pub(super) async fn composite_keys_arrays_and_numeric(pool: &PgPool) {
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

/// Every unique key other than the primary key gets a `get_by_<cols>`
/// lookup: `products.sku` (made unique by alter_schema.sql) and the
/// composite `(organization_id, code)` of `app.branches`.
pub(super) async fn unique_key_lookups_find_one_row_or_none(pool: &PgPool) {
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

/// A text key's typed id is not `Copy`, so every generated function borrows
/// it; a key struct with a text column is borrowed the same way.
pub(super) async fn text_keys_are_borrowed(pool: &PgPool) {
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
