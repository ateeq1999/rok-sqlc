//! Relationship loaders.

use super::*;

/// Loading `category` for 50 products is one query, keyed by each
/// product's `category_id`.
pub(super) async fn belongs_to_loader_fetches_every_parent_in_one_query(pool: &PgPool) {
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
pub(super) async fn has_many_loader_stays_in_the_tenant_and_skips_deleted_rows(pool: &PgPool) {
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
pub(super) async fn several_foreign_keys_to_one_table_get_distinct_names(pool: &PgPool) {
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
pub(super) async fn unique_foreign_key_loads_one_row(pool: &PgPool) {
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
pub(super) async fn self_reference_loads_parent_and_children(pool: &PgPool) {
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

/// `tests/fixture_compile/` renames `expense_claims.created_by_user` to
/// `author` and its inverse on `users` to `authored_expense_claims`, and opts
/// out `expense_claims.approved_by_user`, which removes both of its sides:
/// the renamed loaders work like inferred ones, and no `load_approved_by_user`
/// or `load_expense_claims_by_approved_by_user` exists to call.
pub(super) async fn renamed_relationships_load_under_their_new_names(pool: &PgPool) {
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
