//! The tenant convention and row-level security.

use super::*;

/// `tenant_notes` is scoped by `tenant_id`: every generated function takes
/// the tenant right after the executor, so a row created under one tenant is
/// invisible to every read, update, and delete made with another. The struct
/// literals below name every field, so `NewTenantNote` and `UpdateTenantNote`
/// have no `tenant_id`.
pub(super) async fn tenant_rows_are_invisible_to_other_tenants(pool: &PgPool) {
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
pub(super) async fn a_tenant_in_the_primary_key_is_passed_once(pool: &PgPool) {
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
pub(super) async fn row_level_security_sees_the_session_tenant(pool: &PgPool) {
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
