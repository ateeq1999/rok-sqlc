use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use tempfile::TempDir;

const SIMPLE_SCHEMA: &str = "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);";
const MIXED_SCHEMA: &str = "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);\n\
     CREATE TABLE money (id BIGSERIAL PRIMARY KEY, amount NUMERIC(12, 2) NOT NULL);";

fn run(args: &[&str]) -> Result<()> {
    let mut full: Vec<String> = vec!["rok-sqlc".to_owned()];
    full.extend(args.iter().map(|arg| arg.to_string()));
    rok_sqlc::cli::run_from(full)
}

fn output_dir(dir: &TempDir) -> PathBuf {
    dir.path().join("out")
}

#[test]
fn generates_resource_modules() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ])?;

    let out = output_dir(&dir);
    assert!(out.join("users/mod.rs").exists());
    assert!(out.join("users/models.rs").exists());
    assert!(out.join("users/read.rs").exists());
    assert!(out.join("mod.rs").exists());
    assert!(out.join("schema.rs").exists());

    let models = fs::read_to_string(out.join("users/models.rs"))?;
    assert!(models.contains("pub struct Users {"));
    assert!(models.contains("pub id: uuid::Uuid,"));
    assert!(models.contains("pub email: String,"));

    let read = fs::read_to_string(out.join("users/read.rs"))?;
    assert!(
        read.contains("SELECT \\\"id\\\", \\\"email\\\" FROM \\\"users\\\" WHERE \\\"id\\\" = $1")
    );

    let db_mod = fs::read_to_string(out.join("mod.rs"))?;
    assert!(db_mod.contains("pub mod users;"));
    assert!(db_mod.contains("\"uuid\""));
    Ok(())
}

#[test]
fn dry_run_writes_nothing() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
        "--dry-run",
    ])?;

    assert!(!output_dir(&dir).exists());
    Ok(())
}

#[test]
fn strict_failure_writes_nothing() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, MIXED_SCHEMA)?;

    let result = run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ]);

    assert!(result.is_err());
    assert!(!output_dir(&dir).exists());
    Ok(())
}

#[test]
fn lenient_skips_unsupported_and_generates_rest() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, MIXED_SCHEMA)?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
        "--lenient",
    ])?;

    let out = output_dir(&dir);
    assert!(out.join("users/models.rs").exists());
    assert!(!out.join("money").exists());
    Ok(())
}

#[test]
fn resource_filter_limits_generation() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(
        &schema,
        format!("{SIMPLE_SCHEMA}\nCREATE TABLE posts (id BIGSERIAL PRIMARY KEY);"),
    )?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
        "--resource",
        "posts",
    ])?;

    let out = output_dir(&dir);
    assert!(out.join("posts/models.rs").exists());
    assert!(!out.join("users").exists());
    Ok(())
}

#[test]
fn unknown_resource_fails() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;

    let result = run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
        "--resource",
        "nope",
    ]);

    assert!(result.is_err());
    Ok(())
}

#[test]
fn overwrite_protection_blocks_foreign_files() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;

    let out = output_dir(&dir);
    let models = out.join("users/models.rs");
    fs::create_dir_all(models.parent().unwrap())?;
    fs::write(&models, "// hand-written, precious\n")?;

    let guarded = run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ]);
    assert!(guarded.is_err());
    assert_eq!(fs::read_to_string(&models)?, "// hand-written, precious\n");
    assert!(!out.join("users/read.rs").exists());

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
        "--force",
    ])?;
    assert!(fs::read_to_string(&models)?.contains("// @generated by rok-sqlc"));
    Ok(())
}

#[test]
fn regeneration_is_idempotent() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;

    let out = output_dir(&dir).to_str().unwrap().to_owned();
    let args = [
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        out.as_str(),
    ];
    run(&args)?;
    let models = fs::read_to_string(output_dir(&dir).join("users/models.rs"))?;
    run(&args)?;
    assert_eq!(
        fs::read_to_string(output_dir(&dir).join("users/models.rs"))?,
        models
    );
    Ok(())
}

#[test]
fn invalid_sql_fails() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, "CREATE TABLE broken (")?;

    let result = run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ]);

    assert!(result.is_err());
    Ok(())
}

#[test]
fn snapshot_escapes_raw_string_delimiters() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(
        &schema,
        format!("{SIMPLE_SCHEMA}\n-- note: \"# inside comment\n"),
    )?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ])?;

    let snapshot = fs::read_to_string(output_dir(&dir).join("schema.rs"))?;
    assert!(snapshot.contains("r##\""));
    Ok(())
}

#[test]
fn filtered_runs_merge_root_mod() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(
        &schema,
        format!("{SIMPLE_SCHEMA}\nCREATE TABLE posts (id BIGSERIAL PRIMARY KEY);"),
    )?;

    let out = output_dir(&dir).to_str().unwrap().to_owned();
    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        &out,
        "--resource",
        "users",
    ])?;
    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        &out,
        "--resource",
        "posts",
    ])?;

    let db_mod = fs::read_to_string(output_dir(&dir).join("mod.rs"))?;
    assert!(db_mod.contains("pub mod users;"));
    assert!(db_mod.contains("pub mod posts;"));
    assert!(output_dir(&dir).join("users/models.rs").exists());
    assert!(output_dir(&dir).join("posts/models.rs").exists());
    Ok(())
}

#[test]
fn binary_dry_run_lists_planned_files() -> Result<()> {
    use std::process::Command;

    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;

    let output = Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
        .args([
            "generate",
            "--schema",
            schema.to_str().unwrap(),
            "--output",
            dir.path().join("out").to_str().unwrap(),
            "--dry-run",
        ])
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("would write"));
    assert!(!dir.path().join("out").exists());
    Ok(())
}

#[test]
fn missing_schema_file_fails() {
    let result = run(&["generate", "--schema", "does-not-exist.sql"]);
    assert!(result.is_err());
}
