//! Migration directories and schema inputs: order, and file and line
//! diagnostics.

use super::*;

/// Generates from `args` (paths already absolute) into `out`, returning the
/// schema snapshot.
fn generate_snapshot(schema_args: &[&Path], out: &Path) -> Result<String> {
    let mut args = vec!["generate".to_owned()];
    for path in schema_args {
        args.push("--schema".to_owned());
        args.push(path.to_str().unwrap().to_owned());
    }
    args.push("--output".to_owned());
    args.push(out.to_str().unwrap().to_owned());
    run(&args.iter().map(String::as_str).collect::<Vec<_>>())?;
    Ok(fs::read_to_string(out.join("schema.rs"))?)
}

#[test]
fn migration_directory_generates_from_up_migrations_in_filename_order() -> Result<()> {
    let dir = TempDir::new()?;
    let migrations = dir.path().join("migrations");
    fs::create_dir(&migrations)?;
    fs::write(
        migrations.join("0002_posts.up.sql"),
        "CREATE TABLE posts (id UUID PRIMARY KEY, author_id UUID NOT NULL REFERENCES users (id));\n",
    )?;
    fs::write(
        migrations.join("0002_posts.down.sql"),
        "DROP TABLE posts;\n",
    )?;
    fs::write(
        migrations.join("0001_users.up.sql"),
        "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);\n",
    )?;
    // A down migration that would fail generation if it were read.
    fs::write(
        migrations.join("0001_users.down.sql"),
        "CREATE TABLE spans (id BIGSERIAL PRIMARY KEY, duration POINT NOT NULL);\n",
    )?;
    let out = output_dir(&dir);

    let snapshot = generate_snapshot(&[&migrations], &out)?;

    assert!(out.join("users/models.rs").exists());
    assert!(out.join("posts/models.rs").exists());
    assert!(!out.join("spans").exists());
    assert!(!snapshot.contains("DROP TABLE"), "{snapshot}");
    let users = snapshot.find("CREATE TABLE users").unwrap();
    let posts = snapshot.find("CREATE TABLE posts").unwrap();
    assert!(users < posts, "migrations applied in filename order");
    Ok(())
}

#[test]
fn repeated_schema_flags_combine_inputs_in_the_order_given() -> Result<()> {
    let dir = TempDir::new()?;
    let shared = dir.path().join("shared.sql");
    let module = dir.path().join("module");
    fs::create_dir(&module)?;
    fs::write(&shared, "CREATE TABLE users (id UUID PRIMARY KEY);")?;
    fs::write(
        module.join("0001_posts.up.sql"),
        "CREATE TABLE posts (id UUID PRIMARY KEY, author_id UUID NOT NULL REFERENCES users (id));",
    )?;
    let out = output_dir(&dir);

    let snapshot = generate_snapshot(&[&shared, &module], &out)?;

    assert!(out.join("users/models.rs").exists());
    assert!(out.join("posts/models.rs").exists());
    let users = snapshot.find("CREATE TABLE users").unwrap();
    let posts = snapshot.find("CREATE TABLE posts").unwrap();
    assert!(users < posts, "inputs combined in the order given");

    let reversed = generate_snapshot(&[&module, &shared], &dir.path().join("reversed"))?;
    assert!(
        reversed.find("CREATE TABLE posts").unwrap() < reversed.find("CREATE TABLE users").unwrap()
    );
    Ok(())
}

#[test]
fn unsupported_table_in_a_migration_cites_its_file_and_line() -> Result<()> {
    let dir = TempDir::new()?;
    let migrations = dir.path().join("migrations");
    fs::create_dir(&migrations)?;
    fs::write(
        migrations.join("0001_users.up.sql"),
        "CREATE TABLE users (id UUID PRIMARY KEY);\n",
    )?;
    fs::write(
        migrations.join("0002_tags.up.sql"),
        "CREATE TABLE tags (id UUID PRIMARY KEY);\n",
    )?;
    let third = migrations.join("0003_spans.up.sql");
    fs::write(
        &third,
        "-- Spans measure durations.\n\
         \n\
         CREATE INDEX tags_id ON tags (id);\n\
         \n\
         CREATE TABLE spans (\n\
         \x20   id BIGSERIAL PRIMARY KEY,\n\
         \x20   duration POINT NOT NULL\n\
         );\n",
    )?;
    let out = output_dir(&dir);
    let args = |lenient: bool| {
        let mut args = vec![
            "generate",
            "--schema",
            migrations.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ];
        if lenient {
            args.push("--lenient");
        }
        run_binary(&args)
    };
    let located = format!("{}:5: table `spans`", shown(&third));

    let strict = args(false);
    assert!(!strict.status.success());
    let stderr = String::from_utf8_lossy(&strict.stderr);
    assert!(stderr.contains(&format!("error: {located}: ")), "{stderr}");

    let lenient = args(true);
    assert!(lenient.status.success());
    let stderr = String::from_utf8_lossy(&lenient.stderr);
    assert!(
        stderr.contains(&format!("warning: {located} skipped: ")),
        "{stderr}"
    );
    Ok(())
}

/// `ALTER TABLE` forms neither the SQL parser nor rok-sqlc can read fail
/// only the table they alter, at their `path:line`: a lenient run skips it
/// and parses on, and a strict run fails naming it. Readable forms around
/// them (`SET STATISTICS`, `DROP IDENTITY`) leave their tables generating.
#[test]
fn an_unparsable_alter_table_fails_only_its_table() -> Result<()> {
    let dir = TempDir::new()?;
    let migrations = dir.path().join("migrations");
    fs::create_dir_all(&migrations)?;
    fs::write(
        migrations.join("0001_init.sql"),
        "CREATE TABLE tags (id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY, label TEXT NOT NULL);\n\
         CREATE TABLE notes (id UUID PRIMARY KEY, body TEXT NOT NULL);\n",
    )?;
    fs::write(
        migrations.join("0002_alter.sql"),
        "ALTER TABLE notes ADD COLUMN title TEXT;\n\
         ALTER TABLE tags\n    SET SCHEMA archive;\n\
         CREATE TABLE pins (id UUID PRIMARY KEY, label TEXT);\n\
         ALTER TABLE notes ATTACH PARTITION notes_2026 FOR VALUES IN (1);\n\
         ALTER TABLE unknown ALTER COLUMN id DROP IDENTITY;\n\
         ALTER TABLE pins ALTER COLUMN label SET STATISTICS 100;\n",
    )?;
    let out = output_dir(&dir);
    let args = |lenient: bool| {
        let mut args = vec![
            "g",
            "-s",
            migrations.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ];
        if lenient {
            args.push("--lenient");
        }
        run_binary(&args)
    };
    let path = shown(&migrations.join("0002_alter.sql"));

    let strict = args(false);
    let stderr = String::from_utf8_lossy(&strict.stderr);
    assert!(!strict.status.success(), "{stderr}");
    assert!(
        stderr.contains(&format!("{path}:2: table `tags`: ")),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("{path}:5: table `notes`: ")),
        "{stderr}"
    );
    assert!(stderr.contains("2 unsupported table(s)"), "{stderr}");
    assert!(!out.exists());

    let lenient = args(true);
    let stderr = String::from_utf8_lossy(&lenient.stderr);
    assert!(lenient.status.success(), "{stderr}");
    assert!(
        stderr.contains(&format!("warning: {path}:2: table `tags` skipped: ")),
        "{stderr}"
    );
    assert!(out.join("pins/models.rs").is_file());
    assert!(!out.join("tags").exists());
    assert!(!out.join("notes").exists());
    Ok(())
}

/// Any other statement the SQL parser cannot read still stops the run, at
/// its `path:line`.
#[test]
fn an_unparsable_statement_stops_the_run_at_its_line() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(
        &schema,
        "CREATE TABLE tags (id UUID PRIMARY KEY);\n\nCREATE TABEL oops (id UUID);\n",
    )?;

    let output = run_binary(&[
        "g",
        "-s",
        schema.to_str().unwrap(),
        "-o",
        output_dir(&dir).to_str().unwrap(),
        "--lenient",
    ]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains(&format!(
            "{}:3: invalid PostgreSQL SQL syntax",
            shown(&schema)
        )) && stderr.contains("found: TABEL"),
        "{stderr}"
    );
    Ok(())
}
