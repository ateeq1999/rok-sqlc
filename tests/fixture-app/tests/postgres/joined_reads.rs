//! Joined reads of a row with its parent.

use super::*;

/// `get_with_category` reads a product and its category in one statement.
/// Both tables have `id` and `name`; each row gets its own values.
pub(super) async fn joined_read_fetches_a_row_with_its_parent_in_one_query(pool: &PgPool) {
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
pub(super) async fn joined_read_of_a_nullable_foreign_key_without_a_parent(pool: &PgPool) {
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
pub(super) async fn joined_read_respects_soft_delete_and_tenant_on_both_sides(pool: &PgPool) {
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

/// Columns named with 62 characters, two of them sharing the first 60, on
/// both sides: every value reads back from its own column, though a
/// `t_`/`r_` prefix would cut their aliases to the same 63 bytes.
pub(super) async fn joined_read_of_columns_near_the_identifier_limit(pool: &PgPool) {
    use db::calibration_readings::models::NewCalibrationReading;
    use db::calibration_readings::relationships::get_with_calibration_station;
    use db::calibration_stations::models::NewCalibrationStation;

    let station = db::calibration_stations::write::create(
        pool,
        NewCalibrationStation {
            reading_taken_by_the_long_running_calibration_station_number_a: "station a".to_owned(),
            reading_taken_by_the_long_running_calibration_station_number_b: "station b".to_owned(),
        },
    )
    .await
    .unwrap();
    let reading = db::calibration_readings::write::create(
        pool,
        NewCalibrationReading {
            calibration_station_id: station.id,
            reading_taken_by_the_long_running_calibration_station_number_a: "reading a".to_owned(),
            reading_taken_by_the_long_running_calibration_station_number_b: "reading b".to_owned(),
        },
    )
    .await
    .unwrap();

    let joined = get_with_calibration_station(pool, reading.id)
        .await
        .unwrap()
        .expect("the reading exists");
    assert_eq!(joined.calibration_reading, reading);
    assert_eq!(joined.calibration_station, station);
}
