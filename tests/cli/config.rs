//! The `rok-sqlc.toml` config file: discovery, resolution, and overrides.

use super::*;

#[test]
fn config_file_in_a_parent_directory_is_discovered_and_resolved_against_its_directory() -> Result<()>
{
    let dir = TempDir::new()?;
    fs::create_dir_all(dir.path().join("db"))?;
    fs::write(dir.path().join("db/schema.sql"), SIMPLE_SCHEMA)?;
    fs::write(
        dir.path().join("rok-sqlc.toml"),
        "schema = [\"db/schema.sql\"]\noutput = \"gen\"\n",
    )?;
    let sub = dir.path().join("crates/app");
    fs::create_dir_all(&sub)?;

    let output = run_binary_in(&sub, &["g"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dir.path().join("gen/users/models.rs").exists());
    assert!(!sub.join("gen").exists());
    Ok(())
}

#[test]
fn explicit_config_file_is_used_instead_of_discovery() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), SIMPLE_SCHEMA)?;
    // The discoverable file points at a schema that doesn't exist.
    fs::write(
        dir.path().join("rok-sqlc.toml"),
        "schema = \"missing.sql\"\noutput = \"wrong\"\n",
    )?;
    fs::create_dir_all(dir.path().join("config"))?;
    fs::write(
        dir.path().join("config/other.toml"),
        "schema = \"../schema.sql\"\noutput = \"../right\"\n",
    )?;

    let output = run_binary_in(dir.path(), &["g", "-c", "config/other.toml"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dir.path().join("right/users/models.rs").exists());
    assert!(!dir.path().join("wrong").exists());
    Ok(())
}

/// The tenant column gets no null check and no sort key variant, so its
/// names cannot collide with another column's: a table whose search names
/// collide only through the tenant column generates once it is the tenant.
#[test]
fn search_names_the_tenant_column_does_not_get_cannot_collide() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(
        dir.path().join("schema.sql"),
        "CREATE TABLE notes (id BIGSERIAL PRIMARY KEY, org_1 UUID, org_1_is_null BOOLEAN NOT NULL);\n\
         CREATE TABLE tags (id BIGSERIAL PRIMARY KEY, org_1 UUID NOT NULL, org1 INT NOT NULL);\n",
    )?;

    let without = run_binary_in(dir.path(), &["g", "-s", "schema.sql", "-o", "plain"]);
    assert!(!without.status.success());
    let stderr = String::from_utf8_lossy(&without.stderr);
    assert!(
        stderr.contains(
            "column `org_1` needs the null-check filter field `org_1_is_null`, which column `org_1_is_null` already uses"
        ),
        "{stderr}"
    );
    assert!(
        stderr.contains("columns `org_1` and `org1` both map to the sort key variant `Org1`"),
        "{stderr}"
    );

    fs::write(
        dir.path().join("rok-sqlc.toml"),
        "schema = \"schema.sql\"\noutput = \"gen\"\n\n[conventions]\ntenant_column = \"org_1\"\n",
    )?;
    let with = run_binary_in(dir.path(), &["g"]);
    assert!(
        with.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&with.stderr)
    );
    let notes = fs::read_to_string(dir.path().join("gen/notes/filter.rs"))?;
    assert!(
        notes.contains("pub org_1_is_null: Vec<Equality<bool>>,"),
        "{notes}"
    );
    assert!(!notes.contains("pub org_1: "), "{notes}");
    let tags = fs::read_to_string(dir.path().join("gen/tags/filter.rs"))?;
    assert!(tags.contains("    Org1,\n"), "{tags}");
    assert!(tags.contains("Self::Org1 => \"org1\","), "{tags}");
    Ok(())
}

#[test]
fn flags_override_the_config_file() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), MIXED_SCHEMA)?;
    let config = dir.path().join("rok-sqlc.toml");
    fs::write(
        &config,
        "schema = \"schema.sql\"\noutput = \"from-file\"\nresource = \"spans\"\n",
    )?;
    let flag_out = output_dir(&dir);

    run(&[
        "g",
        "--config",
        config.to_str().unwrap(),
        "--output",
        flag_out.to_str().unwrap(),
        "--resource",
        "users",
    ])?;

    assert!(flag_out.join("users/models.rs").exists());
    assert!(!flag_out.join("spans").exists());
    assert!(!dir.path().join("from-file").exists());
    Ok(())
}

#[test]
fn unknown_config_key_names_the_key_and_the_file() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), SIMPLE_SCHEMA)?;
    let config = dir.path().join("rok-sqlc.toml");
    // `output` stays valid so that, if the typo were ignored, the run would
    // write into the temp dir rather than the working directory.
    fs::write(
        &config,
        "schema = \"schema.sql\"\noutput = \"gen\"\nresorce = \"users\"\n",
    )?;

    let err = run(&["g", "-c", config.to_str().unwrap()]).unwrap_err();

    let message = format!("{err:#}");
    assert!(message.contains("resorce"), "{message}");
    assert!(message.contains(&shown(&config)), "{message}");
    assert!(!dir.path().join("gen").exists());
    Ok(())
}

#[test]
fn missing_schema_without_a_config_file_is_a_usage_error() -> Result<()> {
    let dir = TempDir::new()?;
    let output = run_binary_in(dir.path(), &["g"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.starts_with("error: "), "{stderr}");
    assert!(stderr.contains("rok-sqlc.toml"), "{stderr}");
    Ok(())
}

#[test]
fn config_file_schema_entry_can_be_a_migration_directory() -> Result<()> {
    let dir = TempDir::new()?;
    let migrations = dir.path().join("db/migrations");
    fs::create_dir_all(&migrations)?;
    fs::write(
        migrations.join("0002_posts.up.sql"),
        "CREATE TABLE posts (id UUID PRIMARY KEY, author_id UUID NOT NULL REFERENCES users (id));\n",
    )?;
    fs::write(
        migrations.join("0001_users.up.sql"),
        "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);\n",
    )?;
    // Would fail generation if the down migration were read.
    fs::write(
        migrations.join("0001_users.down.sql"),
        "CREATE TABLE spans (id BIGSERIAL PRIMARY KEY, duration POINT NOT NULL);\n",
    )?;
    fs::write(
        dir.path().join("rok-sqlc.toml"),
        "schema = [\"db/migrations/\"]\noutput = \"gen\"\n",
    )?;
    let sub = dir.path().join("crates/app");
    fs::create_dir_all(&sub)?;

    let output = run_binary_in(&sub, &["g"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let generated = dir.path().join("gen");
    assert!(generated.join("users/models.rs").exists());
    assert!(generated.join("posts/models.rs").exists());
    assert!(!generated.join("spans").exists());
    let snapshot = fs::read_to_string(generated.join("schema.rs"))?;
    assert!(
        snapshot.find("CREATE TABLE users").unwrap() < snapshot.find("CREATE TABLE posts").unwrap()
    );
    Ok(())
}

#[test]
fn repeated_schema_flags_replace_the_config_files_schema_list() -> Result<()> {
    let dir = TempDir::new()?;
    // The file's schema doesn't exist: using it at all would fail the run.
    fs::write(
        dir.path().join("rok-sqlc.toml"),
        "schema = [\"missing.sql\", \"missing/\"]\noutput = \"gen\"\n",
    )?;
    fs::write(
        dir.path().join("users.sql"),
        "CREATE TABLE users (id UUID PRIMARY KEY);\n",
    )?;
    fs::write(
        dir.path().join("posts.sql"),
        "CREATE TABLE posts (id UUID PRIMARY KEY, author_id UUID NOT NULL REFERENCES users (id));\n",
    )?;

    let output = run_binary_in(
        dir.path(),
        &["g", "-s", "users.sql", "--schema", "posts.sql"],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dir.path().join("gen/users/models.rs").exists());
    assert!(dir.path().join("gen/posts/models.rs").exists());
    Ok(())
}

#[test]
fn config_file_queries_entry_can_be_a_directory() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), SIMPLE_SCHEMA)?;
    fs::create_dir_all(dir.path().join("db/queries"))?;
    fs::write(
        dir.path().join("db/queries/users.sql"),
        "-- name: ListUsers :many\nSELECT id, email FROM users;\n",
    )?;
    fs::write(
        dir.path().join("rok-sqlc.toml"),
        "schema = \"schema.sql\"\nqueries = \"db/queries\"\noutput = \"gen\"\n",
    )?;
    let sub = dir.path().join("crates/app");
    fs::create_dir_all(&sub)?;

    let output = run_binary_in(&sub, &["g"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dir.path().join("gen/queries/users.rs").is_file());
    assert!(dir.path().join("gen/queries/mod.rs").is_file());
    assert!(!dir.path().join("gen/queries.rs").exists());
    Ok(())
}

#[test]
fn repeated_queries_flags_replace_the_config_files_queries_list() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), SIMPLE_SCHEMA)?;
    // The file's queries don't exist: using them at all would fail the run.
    fs::write(
        dir.path().join("rok-sqlc.toml"),
        "schema = \"schema.sql\"\nqueries = [\"missing.sql\", \"missing/\"]\noutput = \"gen\"\n",
    )?;
    fs::write(
        dir.path().join("users.sql"),
        "-- name: ListUsers :many\nSELECT id, email FROM users;\n",
    )?;
    fs::write(
        dir.path().join("emails.sql"),
        "-- name: ListEmails :many\nSELECT email FROM users;\n",
    )?;

    let output = run_binary_in(
        dir.path(),
        &["g", "-q", "users.sql", "--queries", "emails.sql"],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dir.path().join("gen/queries/users.rs").is_file());
    assert!(dir.path().join("gen/queries/emails.rs").is_file());
    Ok(())
}
