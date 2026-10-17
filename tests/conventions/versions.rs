//! The version and soft-delete columns.

use super::*;

fn write_rs(dir: &TempDir, module: &str) -> Result<String> {
    resource_file(dir, module, "write.rs")
}

#[test]
fn version_column_is_managed_and_versions_only_the_tables_that_have_it() -> Result<()> {
    let (plain, plain_config) = project("")?;
    let (versioned, versioned_config) =
        project("[conventions]\nversion_column = \"row_version\"\n")?;

    generate(&plain_config)?;
    generate(&versioned_config)?;

    // The version column is managed even when `managed_columns` omits it.
    assert_eq!(
        struct_fields(&models(&versioned, "documents")?, "UpdateDocument"),
        ["title", "updated_at", "deleted_at", "removed_at"]
    );
    let documents = write_rs(&versioned, "documents")?;
    assert!(
        documents.contains(
            "    id: DocumentId,\n    expected_version: i64,\n    input: UpdateDocument,\n) -> Result<UpdateOutcome<Document>, sqlx::Error>"
        ),
        "{documents}"
    );
    // A table without the column keeps the plain `update`, byte for byte.
    let tags = write_rs(&versioned, "tags")?;
    assert!(
        tags.contains("-> Result<Option<Tag>, sqlx::Error>"),
        "{tags}"
    );
    assert_eq!(tags, write_rs(&plain, "tags")?);
    let db_mod = fs::read_to_string(versioned.path().join("out/mod.rs"))?;
    assert!(db_mod.contains("pub enum UpdateOutcome<T> {"), "{db_mod}");
    let plain_mod = fs::read_to_string(plain.path().join("out/mod.rs"))?;
    assert!(!plain_mod.contains("UpdateOutcome"), "{plain_mod}");
    Ok(())
}

#[test]
fn a_soft_delete_column_that_is_not_a_nullable_timestamp_fails_generation() -> Result<()> {
    for (column, problem) in [
        ("title", "`title` of `documents`"),
        ("row_version", "`row_version` of `documents`"),
    ] {
        let (dir, config) = project(&format!(
            "[conventions]\nsoft_delete_column = \"{column}\"\n"
        ))?;

        let message = format!("{:#}", generate(&config).unwrap_err());

        assert!(message.contains("soft-delete column"), "{message}");
        assert!(message.contains(problem), "{message}");
        assert!(
            message.contains("nullable TIMESTAMP or TIMESTAMPTZ"),
            "{message}"
        );
        assert!(!dir.path().join("out").exists());
    }
    Ok(())
}

#[test]
fn an_empty_version_column_turns_the_preset_convention_off() -> Result<()> {
    let (dir, config) =
        project("[conventions]\npreset = \"rok\"\nversion_column = \"\"\nmanaged_columns = []\n")?;

    generate(&config)?;

    let documents = write_rs(&dir, "documents")?;
    assert!(
        documents.contains("-> Result<Option<Document>, sqlx::Error>"),
        "{documents}"
    );
    assert!(!documents.contains("UpdateOutcome"), "{documents}");
    Ok(())
}

#[test]
fn a_not_null_timestamp_soft_delete_column_fails_generation() -> Result<()> {
    let (dir, config) = project("[conventions]\nsoft_delete_column = \"updated_at\"\n")?;

    let message = format!("{:#}", generate(&config).unwrap_err());

    assert!(message.contains("`updated_at` of `documents`"), "{message}");
    assert!(message.contains("NOT NULL"), "{message}");
    assert!(!dir.path().join("out").exists());
    Ok(())
}

#[test]
fn the_soft_delete_column_is_left_out_of_inputs_and_patches_but_kept_in_rows() -> Result<()> {
    let (dir, config) = project("[conventions]\nsoft_delete_column = \"deleted_at\"\n")?;

    generate(&config)?;

    let models = models(&dir, "documents")?;
    assert!(struct_fields(&models, "Document").contains(&"deleted_at".to_owned()));
    assert_eq!(
        struct_fields(&models, "NewDocument"),
        ["id", "title", "updated_at", "row_version", "removed_at"]
    );
    assert_eq!(
        struct_fields(&models, "UpdateDocument"),
        ["title", "updated_at", "row_version", "removed_at"]
    );
    Ok(())
}
