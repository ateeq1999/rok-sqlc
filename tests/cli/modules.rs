//! Module renames (`[modules]`), from the config file and the JSON payload.

use super::*;

const SAME_NAMED_USERS: &str = "CREATE TABLE core.users (id UUID PRIMARY KEY);
CREATE TABLE audit.users (id BIGSERIAL PRIMARY KEY);
CREATE TABLE posts (
    id BIGSERIAL PRIMARY KEY,
    author_id UUID NOT NULL REFERENCES core.users (id),
    auditor_id BIGINT REFERENCES audit.users (id)
);
";

/// Runs the binary in `dir` (holding `schema.sql`) with `args`, checks both
/// users modules, and returns `posts/models.rs`.
fn renamed_users(dir: &Path, args: &[&str]) -> Result<String> {
    let output = run_binary_in(dir, args);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let out = dir.join("out");
    let users = fs::read_to_string(out.join("users/models.rs"))?;
    assert!(
        users.contains("pub struct UserId(pub uuid::Uuid);"),
        "{users}"
    );
    let audit = fs::read_to_string(out.join("audit_users/models.rs"))?;
    assert!(audit.contains("pub struct AuditUser {"), "{audit}");
    assert!(
        audit.contains("pub struct AuditUserId(pub i64);"),
        "{audit}"
    );
    let read = fs::read_to_string(out.join("audit_users/read.rs"))?;
    assert!(read.contains("FROM audit.users\n"), "{read}");
    let mod_rs = fs::read_to_string(out.join("mod.rs"))?;
    assert!(mod_rs.contains("pub mod audit_users;"), "{mod_rs}");
    Ok(fs::read_to_string(out.join("posts/models.rs"))?)
}

#[test]
fn a_module_rename_generates_same_named_tables_of_two_schemas() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), SAME_NAMED_USERS)?;
    fs::write(
        dir.path().join("rok-sqlc.toml"),
        "schema = \"schema.sql\"\noutput = \"out\"\n\n[modules]\n\"audit.users\" = \"audit_users\"\n",
    )?;

    let posts = renamed_users(dir.path(), &["g"])?;

    assert!(
        posts.contains("use super::super::audit_users::models::AuditUserId;"),
        "{posts}"
    );
    assert!(
        posts.contains("use super::super::users::models::UserId;"),
        "{posts}"
    );
    assert!(posts.contains("    pub author_id: UserId,\n"), "{posts}");
    assert!(
        posts.contains("    pub auditor_id: Option<AuditUserId>,\n"),
        "{posts}"
    );
    Ok(())
}

#[test]
fn a_module_rename_can_come_from_the_json_payload() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), SAME_NAMED_USERS)?;

    let posts = renamed_users(
        dir.path(),
        &[
            "g",
            "--schema",
            "schema.sql",
            "--output",
            "out",
            "--json",
            r#"{"modules": {"audit.users": "audit_users"}}"#,
        ],
    )?;

    assert!(
        posts.contains("    pub auditor_id: Option<AuditUserId>,\n"),
        "{posts}"
    );
    Ok(())
}

#[test]
fn a_module_rename_must_name_a_table_and_a_free_module() -> Result<()> {
    let cases = [
        (
            "\"audit.accounts\" = \"audit_accounts\"",
            "`modules.\"audit.accounts\"` in the configuration names no table of the schema",
        ),
        (
            "\"audit.users\" = \"Audit Users\"",
            "`modules.\"audit.users\"` in the configuration renames the module to `Audit Users`, which is not a snake_case Rust identifier",
        ),
        (
            "\"audit.users\" = \"posts\"",
            "module name `posts` is already used by another table",
        ),
    ];
    for (entry, expected) in cases {
        let dir = TempDir::new()?;
        fs::write(dir.path().join("schema.sql"), SAME_NAMED_USERS)?;
        fs::write(
            dir.path().join("rok-sqlc.toml"),
            format!("schema = \"schema.sql\"\noutput = \"out\"\n\n[modules]\n{entry}\n"),
        )?;

        let output = run_binary_in(dir.path(), &["g"]);

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{entry}");
        assert!(stderr.contains(expected), "{entry}: {stderr}");
        assert!(!dir.path().join("out").exists(), "{entry}");
    }
    Ok(())
}
