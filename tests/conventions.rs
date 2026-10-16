//! The `[conventions]` configuration section, through the CLI: the `rok`
//! preset, explicit keys, validation, and managed columns. The PostgreSQL
//! behavior of managed columns is proven in `tests/fixture-app`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use tempfile::TempDir;

/// A table maintained by a `touch_row`-style trigger, plus one without any
/// convention column.
const SCHEMA: &str = "CREATE TABLE documents (
    id UUID PRIMARY KEY,
    title TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    row_version BIGINT NOT NULL DEFAULT 0,
    deleted_at TIMESTAMPTZ,
    removed_at TIMESTAMPTZ
);
CREATE TABLE tags (id UUID PRIMARY KEY, label TEXT NOT NULL);
";

fn run(args: &[&str]) -> Result<()> {
    let mut full: Vec<String> = vec!["rok-sqlc".to_owned()];
    full.extend(args.iter().map(|arg| arg.to_string()));
    rok_sqlc::cli::run_from(full)
}

/// A temp dir holding `schema.sql` and a `rok-sqlc.toml` that generates it
/// into `out/` with the given extra TOML appended.
fn project(extra_toml: &str) -> Result<(TempDir, PathBuf)> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), SCHEMA)?;
    let config = dir.path().join("rok-sqlc.toml");
    fs::write(
        &config,
        format!("schema = \"schema.sql\"\noutput = \"out\"\n{extra_toml}"),
    )?;
    Ok((dir, config))
}

fn generate(config: &Path) -> Result<()> {
    run(&["g", "-c", config.to_str().unwrap()])
}

/// The field names of `pub struct {name}` in `source`, in order.
fn struct_fields(source: &str, name: &str) -> Vec<String> {
    let start = source
        .find(&format!("pub struct {name} {{"))
        .unwrap_or_else(|| panic!("no struct {name} in:\n{source}"));
    let body = &source[start..];
    let body = &body[body.find('{').unwrap() + 1..body.find("\n}").unwrap()];
    body.lines()
        .filter_map(|line| line.trim().strip_prefix("pub "))
        .map(|field| field.split(':').next().unwrap().to_owned())
        .collect()
}

fn models(dir: &TempDir, module: &str) -> Result<String> {
    Ok(fs::read_to_string(
        dir.path().join("out").join(module).join("models.rs"),
    )?)
}

/// Every file under `root`, keyed by its path relative to `root`.
fn tree(root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, String>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                walk(root, &path, out)?;
            } else {
                out.insert(
                    path.strip_prefix(root)?.to_path_buf(),
                    fs::read_to_string(&path)?,
                );
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

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
        ["title", "deleted_at", "removed_at"]
    );
    Ok(())
}

#[test]
fn explicit_keys_override_the_preset() -> Result<()> {
    let (dir, config) =
        project("[conventions]\npreset = \"rok\"\nmanaged_columns = [\"removed_at\"]\n")?;

    generate(&config)?;

    let models = models(&dir, "documents")?;
    assert_eq!(
        struct_fields(&models, "UpdateDocument"),
        ["title", "updated_at", "row_version", "deleted_at"]
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
        ["title", "updated_at", "row_version", "removed_at"]
    );
    Ok(())
}

#[test]
fn unknown_preset_names_the_known_presets_and_the_file() -> Result<()> {
    let (dir, config) = project("[conventions]\npreset = \"rokk\"\n")?;

    let message = format!("{:#}", generate(&config).unwrap_err());

    assert!(message.contains("rokk"), "{message}");
    assert!(message.contains("`rok`"), "{message}");
    assert!(message.contains(&config.display().to_string()), "{message}");
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
    assert!(message.contains(&config.display().to_string()), "{message}");

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
