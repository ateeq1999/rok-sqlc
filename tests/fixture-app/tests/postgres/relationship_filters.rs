//! Filters that follow relationships.

use super::*;

/// Products filtered by their category's name: exactly the products in a
/// matching category, each once.
pub(super) async fn belongs_to_filter_matches_each_row_once(pool: &PgPool) {
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
pub(super) async fn has_many_filter_matches_each_parent_once(pool: &PgPool) {
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
pub(super) async fn relationship_filters_skip_deleted_and_other_tenant_rows(pool: &PgPool) {
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
pub(super) async fn self_reference_filters_and_nesting_depth(pool: &PgPool) {
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

/// Relationship filters use the configured names: `author` and
/// `authored_expense_claims` filter like inferred relationships, and the
/// opted-out `approved_by_user` has no filter field (only its column's).
pub(super) async fn renamed_relationships_filter_under_their_new_names(pool: &PgPool) {
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
