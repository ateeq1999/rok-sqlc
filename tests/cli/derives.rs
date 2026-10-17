//! `derive = ["serde"]` and the types it cannot derive for.

use super::*;

/// `derive = ["serde"]` works from the config file, the JSON payload, and
/// `-d/--derive`, and each generates the same files.
#[test]
fn serde_derive_from_file_payload_or_flag() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), SIMPLE_SCHEMA)?;
    fs::create_dir_all(dir.path().join("file"))?;
    fs::write(
        dir.path().join("file/rok-sqlc.toml"),
        "schema = \"../schema.sql\"\noutput = \"../from-file\"\nderive = [\"serde\"]\n",
    )?;

    assert_success(&run_binary_in(&dir.path().join("file"), &["g"]));
    assert_success(&run_binary_in(
        dir.path(),
        &[
            "g",
            "-j",
            r#"{"schema":"schema.sql","output":"from-json","derive":["serde"]}"#,
        ],
    ));
    assert_success(&run_binary_in(
        dir.path(),
        &["g", "-s", "schema.sql", "-o", "from-flag", "-d", "serde"],
    ));
    assert_success(&run_binary_in(
        dir.path(),
        &["g", "-s", "schema.sql", "-o", "plain"],
    ));

    let from_file = read_tree(&dir.path().join("from-file"))?;
    let models = &from_file[Path::new("users/models.rs")];
    assert!(
        models.contains("serde::Serialize, serde::Deserialize)]"),
        "{models}"
    );
    assert_eq!(from_file, read_tree(&dir.path().join("from-json"))?);
    assert_eq!(from_file, read_tree(&dir.path().join("from-flag"))?);
    for (path, text) in read_tree(&dir.path().join("plain"))? {
        assert!(!text.contains("serde"), "{} mentions serde", path.display());
    }
    Ok(())
}

/// SQLx's `PgInterval` and `PgTimeTz` implement no serde traits, so with
/// `derive = ["serde"]` a table or query using them fails the run, naming
/// the column; `BYTEA` (`Vec<u8>`) serializes.
#[test]
fn serde_rejects_interval_and_time_with_time_zone() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(
        dir.path().join("schema.sql"),
        "CREATE TABLE spans (id BIGSERIAL PRIMARY KEY, length INTERVAL NOT NULL);\n\
         CREATE TABLE blobs (id BIGSERIAL PRIMARY KEY, body BYTEA NOT NULL);\n\
         CREATE TABLE shifts (id BIGSERIAL PRIMARY KEY, opens TIME WITH TIME ZONE);\n",
    )?;
    // A query's parameters derive serde only in a `Params` struct, taken
    // past three parameters.
    fs::write(
        dir.path().join("queries.sql"),
        "-- name: LongBlobs :many\nSELECT id FROM blobs WHERE id IN ($1, $2, $3) AND $4::interval IS NOT NULL;\n",
    )?;
    fs::write(
        dir.path().join("rows.sql"),
        "-- name: PositionalSpan :many\nSELECT id FROM blobs WHERE $1::interval IS NOT NULL;\n\
         -- name: BlobSpans :many\nSELECT id, '1 day'::interval AS span FROM blobs;\n",
    )?;

    let output = run_binary_in(
        dir.path(),
        &["g", "-s", "schema.sql", "-o", "gen", "-d", "serde"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains(
            "column `length` of `spans` maps to `sqlx::postgres::types::PgInterval`, which implements no serde traits"
        ),
        "{stderr}"
    );
    assert!(!dir.path().join("gen").exists());

    let output = run_binary_in(
        dir.path(),
        &[
            "g",
            "-s",
            "schema.sql",
            "-o",
            "gen",
            "-d",
            "serde",
            "-r",
            "shifts",
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("column `opens` of `shifts` maps to `sqlx::postgres::types::PgTimeTz"),
        "{stderr}"
    );

    let output = run_binary_in(
        dir.path(),
        &[
            "g",
            "-s",
            "schema.sql",
            "-o",
            "gen",
            "-d",
            "serde",
            "-r",
            "blobs",
            "-q",
            "queries.sql",
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains(
            "parameter `param_4` of query `LongBlobs` maps to `sqlx::postgres::types::PgInterval`"
        ),
        "{stderr}"
    );
    assert!(!dir.path().join("gen").exists());

    let output = run_binary_in(
        dir.path(),
        &[
            "g",
            "-s",
            "schema.sql",
            "-o",
            "gen",
            "-d",
            "serde",
            "-r",
            "blobs",
            "-q",
            "rows.sql",
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains(
            "column `span` of query `BlobSpans` maps to `Option<sqlx::postgres::types::PgInterval>`"
        ),
        "{stderr}"
    );

    assert_success(&run_binary_in(
        dir.path(),
        &[
            "g",
            "-s",
            "schema.sql",
            "-o",
            "gen",
            "-d",
            "serde",
            "-r",
            "blobs",
        ],
    ));
    Ok(())
}

#[test]
fn unknown_derive_is_an_error() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), SIMPLE_SCHEMA)?;

    let output = run_binary_in(
        dir.path(),
        &[
            "g",
            "-j",
            r#"{"schema":"schema.sql","output":"gen","derive":["serdes"]}"#,
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("serdes") && stderr.contains("serde"),
        "{stderr}"
    );
    assert!(!dir.path().join("gen").exists());

    let output = run_binary_in(dir.path(), &["g", "-s", "schema.sql", "-d", "serdes"]);
    assert!(!output.status.success());
    Ok(())
}
