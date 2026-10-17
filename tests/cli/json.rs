//! The `-j`/`--json` payload.

use super::*;

const TWO_TABLES: &str = "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);\n\
     CREATE TABLE teams (id UUID PRIMARY KEY, name TEXT NOT NULL);";

#[test]
fn inline_json_payload_generates_the_same_files_as_the_equivalent_flags() -> Result<()> {
    let dir = TempDir::new()?;
    fs::create_dir_all(dir.path().join("migrations"))?;
    fs::write(dir.path().join("migrations/0001_init.sql"), TWO_TABLES)?;

    assert_success(&run_binary_in(
        dir.path(),
        &["g", "-s", "migrations/", "-o", "from-flags"],
    ));
    assert_success(&run_binary_in(
        dir.path(),
        &[
            "g",
            "-j",
            r#"{"schema":["migrations/"],"output":"from-json"}"#,
        ],
    ));

    let from_flags = read_tree(&dir.path().join("from-flags"))?;
    assert!(!from_flags.is_empty());
    assert_eq!(from_flags, read_tree(&dir.path().join("from-json"))?);
    Ok(())
}

#[test]
fn json_payload_file_paths_resolve_against_the_current_directory() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), SIMPLE_SCHEMA)?;
    // The payload file sits in a subdirectory, but its paths are read from
    // the current directory, unlike rok-sqlc.toml.
    fs::create_dir_all(dir.path().join("ci"))?;
    fs::write(
        dir.path().join("ci/config.json"),
        r#"{"schema":"schema.sql","output":"gen"}"#,
    )?;

    assert_success(&run_binary_in(dir.path(), &["g", "-j", "@ci/config.json"]));

    assert!(dir.path().join("gen/users/models.rs").exists());
    assert!(!dir.path().join("ci/gen").exists());
    Ok(())
}

#[test]
fn json_payload_is_read_from_stdin_with_a_dash() -> Result<()> {
    use std::io::Write;
    use std::process::Stdio;

    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), SIMPLE_SCHEMA)?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
        .current_dir(dir.path())
        .args(["generate", "--json", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(br#"{"schema":["schema.sql"],"output":"gen"}"#)?;
    let output = child.wait_with_output()?;

    assert_success(&output);
    assert!(dir.path().join("gen/users/models.rs").exists());
    Ok(())
}

#[test]
fn json_payload_overrides_the_config_file_and_flags_override_the_payload() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), TWO_TABLES)?;
    fs::write(
        dir.path().join("rok-sqlc.toml"),
        "schema = \"schema.sql\"\noutput = \"from-file\"\nresource = \"teams\"\n",
    )?;

    // `schema` comes from the file, `output` from the payload, and
    // `resource` from the flag.
    assert_success(&run_binary_in(
        dir.path(),
        &[
            "g",
            "-j",
            r#"{"output":"from-json","resource":"teams"}"#,
            "-r",
            "users",
        ],
    ));

    assert!(dir.path().join("from-json/users/models.rs").exists());
    assert!(!dir.path().join("from-json/teams").exists());
    assert!(!dir.path().join("from-file").exists());
    Ok(())
}

#[test]
fn unknown_json_key_is_named_and_nothing_is_written() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), SIMPLE_SCHEMA)?;

    let output = run_binary_in(
        dir.path(),
        &[
            "g",
            "-j",
            r#"{"schema":"schema.sql","output":"gen","ouptut":"x"}"#,
        ],
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("JSON payload"), "{stderr}");
    assert!(stderr.contains("unknown field `ouptut`"), "{stderr}");
    assert!(!dir.path().join("gen").exists());
    assert!(!dir.path().join("x").exists());
    Ok(())
}

#[test]
fn malformed_json_says_where_it_broke_and_nothing_is_written() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), SIMPLE_SCHEMA)?;
    fs::write(
        dir.path().join("config.json"),
        "{\"schema\": \"schema.sql\",\n \"output\": \"gen\",,}",
    )?;

    let output = run_binary_in(dir.path(), &["g", "-j", "@config.json"]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("invalid JSON payload in config.json"),
        "{stderr}"
    );
    assert!(stderr.contains("line 2"), "{stderr}");
    assert!(!dir.path().join("gen").exists());
    Ok(())
}
