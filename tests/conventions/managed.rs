//! Managed columns, the `rok` preset, and convention keys from the file and
//! the JSON payload.

use super::*;

#[test]
fn managed_columns_are_left_out_of_inputs_and_patches_but_kept_in_rows() -> Result<()> {
    let (dir, config) =
        project("[conventions]\nmanaged_columns = [\"updated_at\", \"row_version\"]\n")?;

    generate(&config)?;

    let models = models(&dir, "documents")?;
    assert_eq!(
        struct_fields(&models, "Document"),
        [
            "id",
            "title",
            "updated_at",
            "row_version",
            "deleted_at",
            "removed_at"
        ]
    );
    assert_eq!(
        struct_fields(&models, "NewDocument"),
        ["id", "title", "deleted_at", "removed_at"]
    );
    assert_eq!(
        struct_fields(&models, "UpdateDocument"),
        ["title", "deleted_at", "removed_at"]
    );
    Ok(())
}

#[test]
fn without_conventions_columns_named_like_conventions_are_ordinary() -> Result<()> {
    let (plain, plain_config) = project("")?;
    let (empty, empty_config) = project("[conventions]\n")?;

    generate(&plain_config)?;
    generate(&empty_config)?;

    let models = models(&plain, "documents")?;
    assert_eq!(
        struct_fields(&models, "NewDocument"),
        [
            "id",
            "title",
            "updated_at",
            "row_version",
            "deleted_at",
            "removed_at"
        ]
    );
    assert_eq!(
        struct_fields(&models, "UpdateDocument"),
        [
            "title",
            "updated_at",
            "row_version",
            "deleted_at",
            "removed_at"
        ]
    );
    assert_eq!(
        tree(&plain.path().join("out"))?,
        tree(&empty.path().join("out"))?
    );
    Ok(())
}

#[test]
fn rok_preset_manages_updated_at_and_row_version() -> Result<()> {
    let (dir, config) = project("[conventions]\npreset = \"rok\"\n")?;

    generate(&config)?;

    let models = models(&dir, "documents")?;
    assert_eq!(
        struct_fields(&models, "UpdateDocument"),
        // `deleted_at` is the preset's soft-delete column.
        ["title", "removed_at"]
    );
    Ok(())
}

/// The preset's version column `row_version` stays managed: the version
/// column always is, whatever `managed_columns` says.
#[test]
fn explicit_keys_override_the_preset() -> Result<()> {
    let (dir, config) = project(
        "[conventions]\npreset = \"rok\"\nmanaged_columns = [\"removed_at\"]\nsoft_delete_column = \"\"\n",
    )?;

    generate(&config)?;

    let models = models(&dir, "documents")?;
    assert_eq!(
        struct_fields(&models, "UpdateDocument"),
        ["title", "updated_at", "deleted_at"]
    );
    Ok(())
}

#[test]
fn json_payload_conventions_override_the_file_key_by_key() -> Result<()> {
    let (dir, config) = project("[conventions]\nmanaged_columns = [\"removed_at\"]\n")?;

    run(&[
        "g",
        "-c",
        config.to_str().unwrap(),
        "-j",
        r#"{"conventions":{"preset":"rok","managed_columns":["deleted_at"]}}"#,
    ])?;

    let models = models(&dir, "documents")?;
    assert_eq!(
        struct_fields(&models, "UpdateDocument"),
        ["title", "updated_at", "removed_at"]
    );
    Ok(())
}

#[test]
fn unknown_preset_names_the_known_presets_and_the_file() -> Result<()> {
    let (dir, config) = project("[conventions]\npreset = \"rokk\"\n")?;

    let message = format!("{:#}", generate(&config).unwrap_err());

    assert!(message.contains("rokk"), "{message}");
    assert!(message.contains("`rok`"), "{message}");
    assert!(message.contains(&shown(&config)), "{message}");
    assert!(!dir.path().join("out").exists());
    Ok(())
}

#[test]
fn unknown_convention_key_names_the_key_and_the_source() -> Result<()> {
    let (dir, config) = project("[conventions]\nsoft_delete_colum = \"deleted_at\"\n")?;

    let message = format!("{:#}", generate(&config).unwrap_err());
    assert!(
        message.contains("unknown field `soft_delete_colum`"),
        "{message}"
    );
    assert!(message.contains("soft_delete_column"), "{message}");
    assert!(message.contains(&shown(&config)), "{message}");

    let message = format!(
        "{:#}",
        run(&[
            "g",
            "-c",
            config.to_str().unwrap(),
            "-j",
            r#"{"conventions":{"tenant":"organization_id"}}"#,
        ])
        .unwrap_err()
    );
    assert!(message.contains("JSON payload"), "{message}");
    assert!(message.contains("unknown field `tenant`"), "{message}");
    assert!(!dir.path().join("out").exists());
    Ok(())
}
