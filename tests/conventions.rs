//! The `[conventions]` configuration section, through the CLI: the `rok`
//! preset, explicit keys, validation, and managed columns. The PostgreSQL
//! behavior of managed columns is proven in `tests/fixture-app`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use tempfile::TempDir;

mod common;

use common::shown;

/// A table maintained by a `touch_row`-style trigger, one without any
/// convention column, and two with an `organization_id` tenant column (one
/// with it in the primary key).
const SCHEMA: &str = "CREATE TABLE documents (
    id UUID PRIMARY KEY,
    title TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    row_version BIGINT NOT NULL DEFAULT 0,
    deleted_at TIMESTAMPTZ,
    removed_at TIMESTAMPTZ
);
CREATE TABLE tags (id UUID PRIMARY KEY, label TEXT NOT NULL);
CREATE TABLE notes (
    id UUID PRIMARY KEY,
    organization_id UUID NOT NULL,
    code TEXT NOT NULL,
    body TEXT NOT NULL,
    UNIQUE (organization_id, code)
);
CREATE TABLE members (
    organization_id UUID NOT NULL,
    id UUID NOT NULL,
    email TEXT NOT NULL,
    PRIMARY KEY (organization_id, id)
);
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
    resource_file(dir, module, "models.rs")
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

fn resource_file(dir: &TempDir, module: &str, file: &str) -> Result<String> {
    Ok(fs::read_to_string(
        dir.path().join("out").join(module).join(file),
    )?)
}

/// Every `pub async fn` signature in `source`, as `name(params)` on one line.
fn signatures(source: &str) -> Vec<String> {
    source
        .split("pub async fn ")
        .skip(1)
        .map(|rest| {
            let name = rest.split(['<', '(']).next().unwrap();
            let params = &rest[rest.find('(').unwrap() + 1..rest.find(") ->").unwrap()];
            let params: Vec<&str> = params
                .split(",\n")
                .flat_map(|chunk| chunk.split(", "))
                .map(str::trim)
                .filter(|param| !param.is_empty())
                .collect();
            format!("{name}({})", params.join(", "))
        })
        .collect()
}

const TENANT: &str = "[conventions]\ntenant_column = \"organization_id\"\n";

#[test]
fn tenant_tables_take_the_tenant_right_after_the_executor() -> Result<()> {
    let (dir, config) = project(TENANT)?;

    generate(&config)?;

    let read = signatures(&resource_file(&dir, "notes", "read.rs")?);
    let write = signatures(&resource_file(&dir, "notes", "write.rs")?);
    assert_eq!(
        read,
        [
            "get_by_id(executor: E, organization_id: uuid::Uuid, id: NoteId)",
            "exists_by_id(executor: E, organization_id: uuid::Uuid, id: NoteId)",
            "get_by_code(executor: E, organization_id: uuid::Uuid, code: &str)",
            "list(executor: E, organization_id: uuid::Uuid)",
            "list_page(executor: E, organization_id: uuid::Uuid, limit: i64, offset: i64)",
            "count(executor: E, organization_id: uuid::Uuid)",
            "search(executor: E, organization_id: uuid::Uuid, filter: NoteFilter, sort: Sort<NoteSortKey>, page: Page)",
            "search_after(executor: E, organization_id: uuid::Uuid, filter: NoteFilter, after: Option<NoteId>, limit: Limit)",
        ]
    );
    assert_eq!(
        write,
        [
            "create(executor: E, organization_id: uuid::Uuid, input: NewNote)",
            "update(executor: E, organization_id: uuid::Uuid, id: NoteId, input: UpdateNote)",
            "delete(executor: E, organization_id: uuid::Uuid, id: NoteId)",
        ]
    );

    let models = models(&dir, "notes")?;
    assert_eq!(
        struct_fields(&models, "Note"),
        ["id", "organization_id", "code", "body"]
    );
    assert_eq!(struct_fields(&models, "NewNote"), ["id", "code", "body"]);
    assert_eq!(struct_fields(&models, "UpdateNote"), ["code", "body"]);
    Ok(())
}

#[test]
fn a_primary_key_including_the_tenant_takes_the_tenant_once() -> Result<()> {
    let (dir, config) = project(TENANT)?;

    generate(&config)?;

    let read = signatures(&resource_file(&dir, "members", "read.rs")?);
    assert_eq!(
        read[0],
        "get_by_id(executor: E, organization_id: uuid::Uuid, id: uuid::Uuid)"
    );
    assert!(
        read.iter()
            .all(|signature| signature.matches("organization_id").count() == 1),
        "{read:?}"
    );
    let write = signatures(&resource_file(&dir, "members", "write.rs")?);
    assert_eq!(
        write,
        [
            "create(executor: E, organization_id: uuid::Uuid, input: NewMember)",
            "update(executor: E, organization_id: uuid::Uuid, id: uuid::Uuid, input: UpdateMember)",
            "delete(executor: E, organization_id: uuid::Uuid, id: uuid::Uuid)",
        ]
    );
    let models = models(&dir, "members")?;
    assert_eq!(struct_fields(&models, "NewMember"), ["id", "email"]);
    assert!(!models.contains("MemberKey"), "{models}");
    Ok(())
}

#[test]
fn without_the_tenant_convention_signatures_are_unchanged() -> Result<()> {
    let (plain, plain_config) = project("")?;
    let (tenant, tenant_config) = project(TENANT)?;

    generate(&plain_config)?;
    generate(&tenant_config)?;

    assert_eq!(
        signatures(&resource_file(&plain, "notes", "read.rs")?)[0],
        "get_by_id(executor: E, id: NoteId)"
    );
    assert_eq!(
        signatures(&resource_file(&plain, "members", "read.rs")?)[0],
        "get_by_id(executor: E, key: MemberKey)"
    );
    // A table without the tenant column is generated as if no convention
    // were configured.
    assert_eq!(
        tree(&plain.path().join("out").join("tags"))?,
        tree(&tenant.path().join("out").join("tags"))?
    );
    assert!(!plain.path().join("out/session.rs").exists());
    assert!(!tenant.path().join("out/session.rs").exists());
    Ok(())
}

#[test]
fn session_settings_generate_transaction_local_helpers() -> Result<()> {
    let (dir, config) = project(
        "[conventions]\ntenant_setting = \"app.organization_id\"\nuser_setting = \"app.user_id\"\n",
    )?;

    generate(&config)?;

    let session = fs::read_to_string(dir.path().join("out/session.rs"))?;
    assert_eq!(
        signatures(&session),
        [
            "set_tenant(connection: &mut sqlx::PgConnection, id: impl std::fmt::Display)",
            "set_user(connection: &mut sqlx::PgConnection, id: impl std::fmt::Display)",
        ]
    );
    assert!(session.contains("SELECT set_config('app.organization_id', $1, true)"));
    assert!(session.contains("SELECT set_config('app.user_id', $1, true)"));
    let db_mod = fs::read_to_string(dir.path().join("out/mod.rs"))?;
    assert!(db_mod.contains("pub mod session;\n"), "{db_mod}");

    let (tenant_only, config) =
        project("[conventions]\ntenant_setting = \"app.organization_id\"\n")?;
    generate(&config)?;
    let session = fs::read_to_string(tenant_only.path().join("out/session.rs"))?;
    assert_eq!(
        signatures(&session),
        ["set_tenant(connection: &mut sqlx::PgConnection, id: impl std::fmt::Display)"]
    );
    Ok(())
}

/// Runs the `rok-sqlc` binary in `dir` over `migrations/` (one file per
/// entry of `files`) with `[conventions]` managing `updated_at`, and returns
/// its stderr and whether it succeeded.
fn generate_migrations(dir: &TempDir, files: &[&str], lenient: bool) -> Result<(bool, String)> {
    let migrations = dir.path().join("migrations");
    fs::create_dir_all(&migrations)?;
    for (index, sql) in files.iter().enumerate() {
        fs::write(migrations.join(format!("{:04}_step.sql", index + 1)), sql)?;
    }
    fs::write(
        dir.path().join("rok-sqlc.toml"),
        "schema = \"migrations\"\noutput = \"out\"\n[conventions]\nmanaged_columns = [\"updated_at\"]\n",
    )?;
    let mut args = vec!["generate"];
    if lenient {
        args.push("--lenient");
    }
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
        .current_dir(dir.path())
        .args(&args)
        .output()?;
    Ok((
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

const UNSOURCED: &str = "CREATE TABLE tags (id UUID PRIMARY KEY, label TEXT NOT NULL);

CREATE TABLE audits (
    id UUID PRIMARY KEY,
    updated_at TIMESTAMPTZ NOT NULL
);
";

#[test]
fn a_not_null_managed_column_with_no_source_of_values_fails_its_table() -> Result<()> {
    let dir = TempDir::new()?;

    let (success, stderr) = generate_migrations(&dir, &[UNSOURCED], false)?;

    assert!(!success, "{stderr}");
    let located = "error: migrations/0001_step.sql:3: table `audits`: managed column `updated_at`";
    assert!(stderr.contains(located), "{stderr}");
    assert!(stderr.contains("add a DEFAULT"), "{stderr}");
    assert!(stderr.contains("BEFORE INSERT trigger"), "{stderr}");
    assert!(!dir.path().join("out").exists());

    let (success, stderr) = generate_migrations(&dir, &[UNSOURCED], true)?;

    assert!(success, "{stderr}");
    assert!(
        stderr.contains("warning: migrations/0001_step.sql:3: table `audits` skipped: "),
        "{stderr}"
    );
    assert!(dir.path().join("out/tags/models.rs").exists());
    assert!(!dir.path().join("out/audits").exists());
    Ok(())
}

#[test]
fn a_managed_column_with_a_source_of_values_generates() -> Result<()> {
    const STAMP: &str = "CREATE FUNCTION stamp() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN NEW.updated_at := now(); RETURN NEW; END $$;
";
    let audits =
        |column: &str| format!("CREATE TABLE audits (id UUID PRIMARY KEY, updated_at {column});\n");
    let not_null = audits("TIMESTAMPTZ NOT NULL");
    let trigger = |events: &str| {
        format!(
            "{not_null}{STAMP}CREATE TRIGGER stamp BEFORE {events} ON audits FOR EACH ROW EXECUTE FUNCTION stamp();\n"
        )
    };
    let sourced = [
        vec![audits("TIMESTAMPTZ NOT NULL DEFAULT now()")],
        vec![audits("BIGINT NOT NULL GENERATED ALWAYS AS IDENTITY")],
        vec![audits("BIGSERIAL")],
        vec![audits("TIMESTAMPTZ")],
        vec![trigger("INSERT")],
        vec![trigger("INSERT OR UPDATE")],
        // The trigger may come in a later migration.
        vec![
            not_null.clone(),
            format!(
                "{STAMP}CREATE TRIGGER stamp BEFORE UPDATE OR INSERT ON audits FOR EACH ROW EXECUTE FUNCTION stamp();"
            ),
        ],
    ];
    for files in sourced {
        let dir = TempDir::new()?;
        let files: Vec<&str> = files.iter().map(String::as_str).collect();

        let (success, stderr) = generate_migrations(&dir, &files, false)?;

        assert!(success, "{files:?}\n{stderr}");
        assert!(dir.path().join("out/audits/models.rs").exists());
    }
    Ok(())
}

#[test]
fn a_trigger_that_cannot_supply_the_value_does_not_count() -> Result<()> {
    const STAMP: &str = "CREATE FUNCTION stamp() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN NEW.updated_at := now(); RETURN NEW; END $$;
";
    let unsourced = [
        format!(
            "{UNSOURCED}{STAMP}CREATE TRIGGER stamp BEFORE UPDATE ON audits FOR EACH ROW EXECUTE FUNCTION stamp();"
        ),
        format!(
            "{UNSOURCED}{STAMP}CREATE TRIGGER stamp AFTER INSERT ON audits FOR EACH ROW EXECUTE FUNCTION stamp();"
        ),
        format!(
            "{UNSOURCED}{STAMP}CREATE TRIGGER stamp BEFORE INSERT ON audits FOR EACH STATEMENT EXECUTE FUNCTION stamp();"
        ),
        format!(
            "{UNSOURCED}{STAMP}CREATE TRIGGER stamp BEFORE INSERT ON tags FOR EACH ROW EXECUTE FUNCTION stamp();"
        ),
        format!(
            "{UNSOURCED}{STAMP}CREATE TRIGGER stamp BEFORE INSERT ON audits FOR EACH ROW EXECUTE FUNCTION stamp();\nDROP TRIGGER stamp ON audits;"
        ),
    ];
    for sql in unsourced {
        let dir = TempDir::new()?;

        let (success, stderr) = generate_migrations(&dir, &[&sql], false)?;

        assert!(!success, "{sql}\n{stderr}");
        assert!(
            stderr.contains("table `audits`: managed column `updated_at`"),
            "{sql}\n{stderr}"
        );
    }
    Ok(())
}
