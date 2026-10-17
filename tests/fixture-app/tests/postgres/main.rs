//! Executes the generated CRUD code against a real PostgreSQL database.
//!
//! Runs only when `ROK_SQLC_TEST_DATABASE_URL` is set. The database is reset
//! (its `public`, `app`, and `legacy` schemas are dropped), so point it at a throwaway
//! database, never one holding data you care about.

mod conventions;
mod joined_reads;
mod keys;
mod loaders;
mod many_to_many;
mod migrations;
mod queries;
mod relationship_filters;
mod search;
mod tenants;
mod types;
mod writes;

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
    sqlx::raw_sql(rok_sqlc_fixture_app::plain_db::schema::SCHEMA_SQL)
        .execute(&pool)
        .await
        .expect("apply the fixture schema generated without serde");
    Some(pool)
}

/// One scenario per test would race on the shared database reset, so the
/// scenarios run in sequence against one fresh database.
#[tokio::test]
async fn generated_crud_against_postgres() {
    let Some(pool) = fresh_database().await else {
        return;
    };
    writes::insert_applies_defaults_and_explicit_nulls(&pool).await;
    writes::every_partial_update_combination_binds_correctly(&pool).await;
    writes::empty_and_missing_updates(&pool).await;
    writes::delete_reports_existing_and_missing_rows(&pool).await;
    writes::transactions_stay_with_the_caller(&pool).await;
    writes::all_defaulted_insert_uses_default_values(&pool).await;
    writes::reserved_and_quoted_identifiers_round_trip(&pool).await;
    writes::table_without_primary_key(&pool).await;
    keys::uuid_keys_listing_and_pagination(&pool).await;
    keys::composite_keys_arrays_and_numeric(&pool).await;
    queries::annotated_queries(&pool).await;
    queries::query_modules_per_file(&pool).await;
    search::search_filters_and_pages(&pool).await;
    migrations::altered_table_has_its_final_columns(&pool).await;
    migrations::altered_columns_round_trip_in_their_final_shape(&pool).await;
    types::enums_keep_their_exact_labels(&pool).await;
    types::renamed_enum_types_use_their_new_name(&pool).await;
    types::value_list_columns_round_trip(&pool).await;
    keys::typed_ids_flow_from_rows_to_foreign_keys(&pool).await;
    keys::enum_keys_get_a_typed_id(&pool).await;
    keys::foreign_keys_resolve_to_the_table_in_their_schema(&pool).await;
    keys::unique_key_lookups_find_one_row_or_none(&pool).await;
    conventions::managed_columns_are_left_to_the_database(&pool).await;
    conventions::versioned_updates_detect_concurrent_edits(&pool).await;
    keys::text_keys_are_borrowed(&pool).await;
    writes::rows_round_trip_through_json(&pool).await;
    writes::patches_from_json_tell_absent_from_null(&pool).await;
    conventions::soft_deleted_rows_are_hidden_restored_and_removed(&pool).await;
    tenants::tenant_rows_are_invisible_to_other_tenants(&pool).await;
    tenants::a_tenant_in_the_primary_key_is_passed_once(&pool).await;
    tenants::row_level_security_sees_the_session_tenant(&pool).await;
    loaders::belongs_to_loader_fetches_every_parent_in_one_query(&pool).await;
    loaders::has_many_loader_stays_in_the_tenant_and_skips_deleted_rows(&pool).await;
    loaders::several_foreign_keys_to_one_table_get_distinct_names(&pool).await;
    loaders::unique_foreign_key_loads_one_row(&pool).await;
    loaders::self_reference_loads_parent_and_children(&pool).await;
    relationship_filters::belongs_to_filter_matches_each_row_once(&pool).await;
    relationship_filters::has_many_filter_matches_each_parent_once(&pool).await;
    relationship_filters::relationship_filters_skip_deleted_and_other_tenant_rows(&pool).await;
    relationship_filters::self_reference_filters_and_nesting_depth(&pool).await;
    loaders::renamed_relationships_load_under_their_new_names(&pool).await;
    joined_reads::joined_read_fetches_a_row_with_its_parent_in_one_query(&pool).await;
    joined_reads::joined_read_of_a_nullable_foreign_key_without_a_parent(&pool).await;
    joined_reads::joined_read_respects_soft_delete_and_tenant_on_both_sides(&pool).await;
    joined_reads::joined_read_of_columns_near_the_identifier_limit(&pool).await;
    relationship_filters::renamed_relationships_filter_under_their_new_names(&pool).await;
    many_to_many::many_to_many_attach_detach_and_sync(&pool).await;
    many_to_many::soft_delete_join_rows_are_restored_not_duplicated(&pool).await;
    many_to_many::plain_join_table_deletes_detached_pairs(&pool).await;
    many_to_many::many_to_many_filters_go_through_the_join_table(&pool).await;
    types::interval_bytea_and_timetz_round_trip(&pool).await;
}

fn skus(rows: &[db::products::models::Product]) -> Vec<String> {
    rows.iter().map(|row| row.sku.clone()).collect()
}

fn base_patch_target() -> NewPatchTarget {
    NewPatchTarget {
        label: "base".to_owned(),
        note: Some("n0".to_owned()),
        score: Some(5),
        tag: Some(Some("t0".to_owned())),
    }
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
