//! Managed columns, versioned updates, and soft delete.

use super::*;

/// `touched_notes` is generated with `updated_at` and `row_version` managed:
/// neither appears in `NewTouchedNote` or `UpdateTouchedNote` (the struct
/// literals below name every field), and the rows carry the values the
/// default and the `touch_row` trigger give them.
pub(super) async fn managed_columns_are_left_to_the_database(pool: &PgPool) {
    use db::touched_notes::models::{NewTouchedNote, UpdateTouchedNote};
    use db::touched_notes::{read, write};

    let created = write::create(
        pool,
        NewTouchedNote {
            body: "first".to_owned(),
        },
    )
    .await
    .unwrap();
    assert_eq!(created.row_version, 1, "the column default applies");

    let updated = write::update(
        pool,
        created.id,
        UpdateTouchedNote {
            body: Some("second".to_owned()),
        },
    )
    .await
    .unwrap()
    .expect("row exists");
    assert_eq!(updated.body, "second");
    assert_eq!(updated.row_version, 2, "the trigger advanced the version");
    assert!(
        updated.updated_at > created.updated_at,
        "the trigger touched updated_at"
    );

    let stored = read::get_by_id(pool, created.id).await.unwrap();
    assert_eq!(stored, Some(updated));
}

/// `versioned_documents` is generated with `revision` as its version column:
/// `update` takes the revision the caller last saw, and the `bump_revision`
/// trigger advances it on every update. `revision` is managed, so neither
/// `NewVersionedDocument` nor `UpdateVersionedDocument` has it (the struct
/// literals below name every field).
pub(super) async fn versioned_updates_detect_concurrent_edits(pool: &PgPool) {
    use db::UpdateOutcome;
    use db::search::{Page, Sort};
    use db::versioned_documents::filter::{VersionedDocumentFilter, VersionedDocumentSortKey};
    use db::versioned_documents::models::{
        NewVersionedDocument, UpdateVersionedDocument, VersionedDocumentId,
    };
    use db::versioned_documents::{read, write};

    let created = write::create(
        pool,
        NewVersionedDocument {
            title: "draft".to_owned(),
            note: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(created.revision, 1);

    // Two editors both saw revision 1; the first one to save wins.
    let first = write::update(
        pool,
        created.id,
        1,
        UpdateVersionedDocument {
            title: Some("first editor".to_owned()),
            note: None,
        },
    )
    .await
    .unwrap();
    let UpdateOutcome::Updated(saved) = first else {
        panic!("the first update should apply, got {first:?}");
    };
    assert_eq!(saved.title, "first editor");
    assert_eq!(saved.revision, 2, "the trigger advanced the revision");

    let second = write::update(
        pool,
        created.id,
        1,
        UpdateVersionedDocument {
            title: Some("second editor".to_owned()),
            note: Some(Some("stale".to_owned())),
        },
    )
    .await
    .unwrap();
    assert_eq!(second, UpdateOutcome::VersionConflict);
    let stored = read::get_by_id(pool, created.id).await.unwrap();
    assert_eq!(
        stored,
        Some(saved.clone()),
        "the stale update changed nothing"
    );

    // Inside a caller-owned transaction, with the current revision.
    let mut tx = pool.begin().await.unwrap();
    let retried = write::update(
        &mut *tx,
        created.id,
        2,
        UpdateVersionedDocument {
            title: Some("second editor".to_owned()),
            note: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let UpdateOutcome::Updated(retried) = retried else {
        panic!("an update at the current revision should apply, got {retried:?}");
    };
    assert_eq!(retried.revision, 3);

    // An empty patch returns the current row without bumping the revision,
    // and still reports a stale revision as a conflict.
    let unchanged = write::update(pool, created.id, 3, UpdateVersionedDocument::default())
        .await
        .unwrap();
    assert_eq!(unchanged, UpdateOutcome::Updated(retried.clone()));
    let stale_empty = write::update(pool, created.id, 2, UpdateVersionedDocument::default())
        .await
        .unwrap();
    assert_eq!(stale_empty, UpdateOutcome::VersionConflict);
    let stored = read::get_by_id(pool, created.id).await.unwrap();
    assert_eq!(stored.map(|row| row.revision), Some(3));

    // A key no row has is not found, with or without a patch.
    let missing = VersionedDocumentId::from(i64::from(created.id) + 1_000);
    let patched_missing = write::update(
        pool,
        missing,
        1,
        UpdateVersionedDocument {
            title: Some("ghost".to_owned()),
            note: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(patched_missing, UpdateOutcome::NotFound);
    let empty_missing = write::update(pool, missing, 1, UpdateVersionedDocument::default())
        .await
        .unwrap();
    assert_eq!(empty_missing, UpdateOutcome::NotFound);

    // A soft-deleted row counts as missing, even at its current revision,
    // and the update leaves it unchanged.
    assert!(write::delete(pool, created.id).await.unwrap());
    let deleted = read::search(
        pool,
        VersionedDocumentFilter {
            include_deleted: true,
            ..VersionedDocumentFilter::default()
        },
        Sort::asc(VersionedDocumentSortKey::Id),
        Page::first(10),
    )
    .await
    .unwrap()
    .remove(0);
    assert!(deleted.deleted_at.is_some());
    let patched_deleted = write::update(
        pool,
        created.id,
        deleted.revision,
        UpdateVersionedDocument {
            title: Some("revived".to_owned()),
            note: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(patched_deleted, UpdateOutcome::NotFound);
    let empty_deleted = write::update(
        pool,
        created.id,
        deleted.revision,
        UpdateVersionedDocument::default(),
    )
    .await
    .unwrap();
    assert_eq!(empty_deleted, UpdateOutcome::NotFound);
    assert!(write::restore(pool, created.id).await.unwrap());
    let restored = read::get_by_id(pool, created.id).await.unwrap().unwrap();
    assert_eq!(
        restored.title, "second editor",
        "the update changed nothing"
    );
}

/// `soft_notes` is generated with `deleted_at` as its soft-delete column:
/// `NewSoftNote` and `UpdateSoftNote` have no `deleted_at` (the struct
/// literals below name every field), `delete` hides a row from every read,
/// `restore` brings it back, and `delete_permanently` removes it.
pub(super) async fn soft_deleted_rows_are_hidden_restored_and_removed(pool: &PgPool) {
    use db::search::{Limit, Page, Sort};
    use db::soft_notes::filter::{SoftNoteFilter, SoftNoteSortKey};
    use db::soft_notes::models::{NewSoftNote, SoftNote, UpdateSoftNote};
    use db::soft_notes::{read, write};

    let create = |code: &str| {
        write::create(
            pool,
            NewSoftNote {
                code: code.to_owned(),
                body: format!("note {code}"),
            },
        )
    };
    let note = create("A").await.unwrap();
    let other = create("B").await.unwrap();
    assert_eq!(note.deleted_at, None);

    let ids = |rows: Vec<SoftNote>| rows.into_iter().map(|row| row.id).collect::<Vec<_>>();
    let search = |filter: SoftNoteFilter| {
        read::search(
            pool,
            filter,
            Sort::asc(SoftNoteSortKey::Id),
            Page::first(10),
        )
    };
    let assert_visible = async |visible: bool| {
        let expect = |ids: Vec<_>| {
            if visible {
                assert_eq!(ids, vec![note.id, other.id]);
            } else {
                assert_eq!(ids, vec![other.id]);
            }
        };
        assert_eq!(
            read::get_by_id(pool, note.id).await.unwrap().is_some(),
            visible
        );
        assert_eq!(
            read::get_by_code(pool, "A").await.unwrap().is_some(),
            visible
        );
        assert_eq!(read::exists_by_id(pool, note.id).await.unwrap(), visible);
        expect(ids(read::list(pool).await.unwrap()));
        expect(ids(read::list_page(pool, 10, 0).await.unwrap()));
        assert_eq!(
            read::count(pool).await.unwrap(),
            if visible { 2 } else { 1 }
        );
        expect(ids(search(SoftNoteFilter::default()).await.unwrap()));
        expect(ids(read::search_after(
            pool,
            SoftNoteFilter::default(),
            None,
            Limit::new(10),
        )
        .await
        .unwrap()));
    };
    assert_visible(true).await;

    assert!(write::delete(pool, note.id).await.unwrap());
    assert!(
        !write::delete(pool, note.id).await.unwrap(),
        "an already-deleted row does not change"
    );
    assert_visible(false).await;
    let patch = UpdateSoftNote {
        code: None,
        body: Some("edited".to_owned()),
    };
    assert_eq!(write::update(pool, note.id, patch).await.unwrap(), None);
    let empty = UpdateSoftNote {
        code: None,
        body: None,
    };
    assert_eq!(write::update(pool, note.id, empty).await.unwrap(), None);

    let with_deleted = search(SoftNoteFilter {
        include_deleted: true,
        ..SoftNoteFilter::default()
    })
    .await
    .unwrap();
    assert_eq!(ids(with_deleted.clone()), vec![note.id, other.id]);
    assert!(with_deleted[0].deleted_at.is_some());
    assert_eq!(with_deleted[0].body, "note A", "the update changed nothing");

    assert!(write::restore(pool, note.id).await.unwrap());
    assert!(
        !write::restore(pool, note.id).await.unwrap(),
        "already live"
    );
    assert_visible(true).await;

    // The partial unique index holds among live rows only: a deleted row and
    // a live row share a code, and `get_by_code` finds the live one.
    assert!(write::delete(pool, note.id).await.unwrap());
    let replacement = create("A").await.unwrap();
    assert_eq!(
        read::get_by_code(pool, "A")
            .await
            .unwrap()
            .map(|row| row.id),
        Some(replacement.id)
    );

    assert!(write::delete_permanently(pool, note.id).await.unwrap());
    assert!(!write::delete_permanently(pool, note.id).await.unwrap());
    let remaining = search(SoftNoteFilter {
        include_deleted: true,
        ..SoftNoteFilter::default()
    })
    .await
    .unwrap();
    assert_eq!(ids(remaining), vec![other.id, replacement.id]);
}
