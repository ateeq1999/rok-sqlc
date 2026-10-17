//! Tables and columns in the final shape their migrations give them.

use super::*;

/// `revised_notes` is created in extra_schema.sql and altered in
/// alter_schema.sql; the generated model carries only the final columns.
pub(super) async fn altered_table_has_its_final_columns(pool: &PgPool) {
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

/// `meter_readings` is reshaped by alter_schema.sql's `ALTER COLUMN`
/// changes; the generated code binds and reads every column in its final
/// shape: widened `BIGINT` key and amount, a `TEXT` unique key, a `label`
/// with no default, a nullable `note`, and a defaulted `views`.
pub(super) async fn altered_columns_round_trip_in_their_final_shape(pool: &PgPool) {
    use db::meter_readings::models::{MeterReading, MeterReadingId, NewMeterReading};
    use db::meter_readings::{read, write};

    // Both exceed `i32`, which the columns were created as.
    let id = MeterReadingId::from(5_000_000_001_i64);
    let amount = 9_000_000_000_i64;
    let created = write::create(
        pool,
        NewMeterReading {
            id,
            code: Some("M-1".to_owned()),
            amount,
            label: "kitchen".to_owned(),
            note: None,
            views: None,
        },
    )
    .await
    .unwrap();
    let expected = MeterReading {
        id,
        code: Some("M-1".to_owned()),
        amount,
        label: "kitchen".to_owned(),
        note: None,
        views: 0,
    };
    assert_eq!(created, expected);
    assert_eq!(read::get_by_id(pool, id).await.unwrap(), Some(expected.clone()));
    assert_eq!(read::get_by_code(pool, "M-1").await.unwrap(), Some(expected));

    // `label` lost its default, so the database now requires it, as the
    // generated `NewMeterReading` does.
    let missing_label = sqlx::query("INSERT INTO meter_readings (id, amount, views) VALUES (1, 1, 1)")
        .execute(pool)
        .await
        .unwrap_err();
    assert!(
        missing_label.to_string().contains("label"),
        "{missing_label}"
    );
}
