//! Inserts, partial updates, deletes, transactions, and JSON round trips.

use super::*;

pub(super) async fn insert_applies_defaults_and_explicit_nulls(pool: &PgPool) {
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
    assert_eq!(
        defaults.tag.as_deref(),
        Some("none"),
        "omitted tag uses DEFAULT 'none'"
    );
    assert_eq!(defaults.note, None);
    assert_eq!(
        defaults.doubled,
        Some(0),
        "generated column is computed by the database"
    );

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
    assert_eq!(
        explicit.tag, None,
        "Some(None) inserts NULL instead of the default"
    );
    assert_eq!(explicit.note.as_deref(), Some("n"));
    assert_eq!(explicit.doubled, Some(8));
}

/// Applies every combination of unchanged / set / set-NULL over the four
/// updatable columns and checks the stored row field by field. A placeholder
/// or bind-order mistake surfaces as a value landing in the wrong column or
/// as a type error from PostgreSQL.
pub(super) async fn every_partial_update_combination_binds_correctly(pool: &PgPool) {
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

pub(super) async fn empty_and_missing_updates(pool: &PgPool) {
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

pub(super) async fn delete_reports_existing_and_missing_rows(pool: &PgPool) {
    use db::patch_targets::{read, write};

    let row = write::create(pool, base_patch_target()).await.unwrap();
    assert!(read::exists_by_id(pool, row.id).await.unwrap());
    assert!(
        write::delete(pool, row.id).await.unwrap(),
        "existing row is deleted"
    );
    assert!(
        !write::delete(pool, row.id).await.unwrap(),
        "second delete finds nothing"
    );
    assert!(!read::exists_by_id(pool, row.id).await.unwrap());
    assert_eq!(read::get_by_id(pool, row.id).await.unwrap(), None);
}

pub(super) async fn transactions_stay_with_the_caller(pool: &PgPool) {
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

pub(super) async fn all_defaulted_insert_uses_default_values(pool: &PgPool) {
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

pub(super) async fn reserved_and_quoted_identifiers_round_trip(pool: &PgPool) {
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
    assert_eq!(
        row.mixed_case, 1,
        "quoted defaulted column applies its default"
    );
    assert_eq!(row.type_.as_deref(), Some("kind"));
    assert_eq!(row._1st, Some(true));

    let patch = UpdateReservedWord {
        type_: Some(None),
        user: Some(Some("alice".to_owned())),
        mixed_case: Some(9),
        ..Default::default()
    };
    let updated = write::update(pool, row.select, patch)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.type_, None);
    assert_eq!(updated.user.as_deref(), Some("alice"));
    assert_eq!(updated.mixed_case, 9);
    assert_eq!(updated.from, "origin");

    assert_eq!(read::count(pool).await.unwrap(), 1);
    assert_eq!(read::list_page(pool, 10, 0).await.unwrap(), vec![updated]);
    assert!(write::delete(pool, row.select).await.unwrap());
}

pub(super) async fn table_without_primary_key(pool: &PgPool) {
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

/// The fixture generates with `derive = ["serde"]`: rows read from the
/// database serialize to JSON and back unchanged. Typed ids serialize as
/// their inner value and enum variants as their database label.
pub(super) async fn rows_round_trip_through_json(pool: &PgPool) {
    use db::products::models::Product;
    use db::tickets::models::Ticket;

    let products = db::products::read::list(pool).await.unwrap();
    assert!(!products.is_empty(), "earlier scenarios left products");
    for product in &products {
        let json = serde_json::to_string(product).unwrap();
        assert_eq!(&serde_json::from_str::<Product>(&json).unwrap(), product);
    }
    let value = serde_json::to_value(&products[0]).unwrap();
    assert_eq!(value["id"], serde_json::json!(products[0].id.0));

    let tickets = db::tickets::read::list(pool).await.unwrap();
    let very_high = tickets
        .iter()
        .find(|ticket| ticket.priority == db::types::TicketPriority::VeryHigh)
        .expect("earlier scenarios left a very high ticket");
    let json = serde_json::to_value(very_high).unwrap();
    assert_eq!(json["priority"], "Very High");
    assert_eq!(&serde_json::from_value::<Ticket>(json).unwrap(), very_high);
}

/// A patch deserialized from JSON leaves the columns absent from it alone
/// and sets those given as `null` to `NULL`; serializing skips absent fields.
pub(super) async fn patches_from_json_tell_absent_from_null(pool: &PgPool) {
    use db::patch_targets::write;

    let before = write::create(pool, base_patch_target()).await.unwrap();
    assert_eq!(before.note.as_deref(), Some("n0"));

    let untouched: UpdatePatchTarget = serde_json::from_str("{}").unwrap();
    assert!(untouched.is_empty());
    assert_eq!(serde_json::to_string(&untouched).unwrap(), "{}");
    let row = write::update(pool, before.id, untouched)
        .await
        .unwrap()
        .expect("row exists");
    assert_eq!(row, before, "an empty patch changes nothing");

    let cleared: UpdatePatchTarget =
        serde_json::from_str(r#"{"note": null, "score": 9}"#).unwrap();
    assert_eq!(cleared.note, Some(None));
    assert_eq!(cleared.tag, None);
    assert_eq!(
        serde_json::to_value(&cleared).unwrap(),
        serde_json::json!({"note": null, "score": 9})
    );
    let row = write::update(pool, before.id, cleared)
        .await
        .unwrap()
        .expect("row exists");
    assert_eq!(row.note, None, "`null` sets the column to NULL");
    assert_eq!(row.score, 9);
    assert_eq!(row.tag, before.tag, "an absent column is left alone");
    assert_eq!(row.label, before.label);

    let set: UpdatePatchTarget = serde_json::from_str(r#"{"note": "again"}"#).unwrap();
    assert_eq!(set.note, Some(Some("again".to_owned())));
}
