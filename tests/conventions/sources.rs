//! Where a managed column's values come from: defaults and triggers.

use super::*;

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

    // Dropping the last trigger in a later migration fails the table there.
    let dir = TempDir::new()?;
    let created = format!(
        "{UNSOURCED}{STAMP}CREATE TRIGGER stamp BEFORE INSERT ON audits FOR EACH ROW EXECUTE FUNCTION stamp();\n"
    );
    let (success, stderr) = generate_migrations(
        &dir,
        &[&created, "\n\nDROP TRIGGER stamp ON audits;\n"],
        false,
    )?;
    assert!(!success, "{stderr}");
    assert!(
        stderr.contains(
            "error: migrations/0002_step.sql:3: table `audits`: managed column `updated_at`"
        ),
        "{stderr}"
    );
    Ok(())
}

/// The source check sees a managed column's final shape: a later migration
/// that drops its default leaves it without a source of values, and one that
/// adds a default or drops NOT NULL gives it one.
#[test]
fn alter_column_changes_whether_a_managed_column_has_a_source_of_values() -> Result<()> {
    const AUDITS: &str = "CREATE TABLE audits (\n    id UUID PRIMARY KEY,\n    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()\n);\n";
    let dir = TempDir::new()?;

    let (success, stderr) = generate_migrations(
        &dir,
        &[
            AUDITS,
            "-- Stop stamping rows.\nALTER TABLE audits ALTER COLUMN updated_at DROP DEFAULT;",
        ],
        false,
    )?;

    assert!(!success, "{stderr}");
    // The diagnostic points at the statement that took the source away.
    let located = "error: migrations/0002_step.sql:2: table `audits`: managed column `updated_at` is NOT NULL with no DEFAULT";
    assert!(stderr.contains(located), "{stderr}");

    // A serial column is NOT NULL, so dropping its sequence default leaves
    // a managed one without a source of values.
    let dir = TempDir::new()?;
    let (success, stderr) = generate_migrations(
        &dir,
        &[
            "CREATE TABLE audits (id UUID PRIMARY KEY, updated_at BIGSERIAL);",
            "ALTER TABLE audits ALTER COLUMN updated_at DROP DEFAULT;",
        ],
        false,
    )?;
    assert!(!success, "{stderr}");
    assert!(
        stderr.contains("table `audits`: managed column `updated_at` is NOT NULL with no DEFAULT"),
        "{stderr}"
    );

    for fix in [
        "ALTER TABLE audits ALTER COLUMN updated_at SET DEFAULT now();",
        "ALTER TABLE audits ALTER COLUMN updated_at DROP NOT NULL;",
    ] {
        let dir = TempDir::new()?;
        let files = [
            AUDITS,
            "ALTER TABLE audits ALTER COLUMN updated_at DROP DEFAULT;",
            fix,
        ];

        let (success, stderr) = generate_migrations(&dir, &files, false)?;

        assert!(success, "{fix}\n{stderr}");
        assert!(dir.path().join("out/audits/models.rs").exists());
    }
    Ok(())
}
