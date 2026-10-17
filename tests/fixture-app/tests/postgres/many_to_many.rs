//! Many-to-many writes and filters through join tables.

use super::*;

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
pub(super) async fn many_to_many_attach_detach_and_sync(pool: &PgPool) {
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
pub(super) async fn soft_delete_join_rows_are_restored_not_duplicated(pool: &PgPool) {
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
pub(super) async fn plain_join_table_deletes_detached_pairs(pool: &PgPool) {
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
pub(super) async fn many_to_many_filters_go_through_the_join_table(pool: &PgPool) {
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
