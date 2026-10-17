//! `derive = ["validate"]` and `extractors` (ADR 0015): where they are set,
//! how layers override them, and the derives each one needs.

use super::*;

/// Runs `rok-sqlc` in `dir` and returns its stderr, asserting it failed and
/// wrote nothing.
fn failure(dir: &Path, args: &[&str]) -> String {
    let output = run_binary_in(dir, args);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!output.status.success(), "succeeded: {args:?}");
    assert!(!dir.join("gen").exists(), "wrote files: {args:?}");
    stderr
}

/// A directory with `schema.sql` and, when `config` is given, a
/// `rok-sqlc.toml` reading it into `gen`.
fn project(config: Option<&str>) -> Result<TempDir> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), SIMPLE_SCHEMA)?;
    if let Some(config) = config {
        fs::write(
            dir.path().join("rok-sqlc.toml"),
            format!("schema = \"schema.sql\"\noutput = \"gen\"\n{config}"),
        )?;
    }
    Ok(dir)
}

const VALIDATE_NEEDS_SERDE: &str = "`validate` in `derive` needs `serde`";
const EXTRACTORS_NEED_VALIDATE: &str = "`extractors` needs `validate` in `derive`";

/// `validate` without `serde` fails the run, wherever `derive` is set.
#[test]
fn validate_without_serde_fails_the_run() -> Result<()> {
    let dir = project(Some("derive = [\"validate\"]\n"))?;
    let stderr = failure(dir.path(), &["g"]);
    assert!(stderr.contains(VALIDATE_NEEDS_SERDE), "{stderr}");

    let dir = project(None)?;
    let stderr = failure(
        dir.path(),
        &[
            "g",
            "-j",
            r#"{"schema":"schema.sql","output":"gen","derive":["validate"]}"#,
        ],
    );
    assert!(stderr.contains(VALIDATE_NEEDS_SERDE), "{stderr}");
    let stderr = failure(
        dir.path(),
        &["g", "-s", "schema.sql", "-o", "gen", "-d", "validate"],
    );
    assert!(stderr.contains(VALIDATE_NEEDS_SERDE), "{stderr}");
    Ok(())
}

/// `extractors` without `validate` fails the run, wherever either is set.
#[test]
fn extractors_without_validate_fail_the_run() -> Result<()> {
    let dir = project(Some(
        "derive = [\"serde\"]\nextractors = [\"axum\", \"actix_web\"]\n",
    ))?;
    let stderr = failure(dir.path(), &["g"]);
    assert!(stderr.contains(EXTRACTORS_NEED_VALIDATE), "{stderr}");

    let dir = project(None)?;
    let stderr = failure(
        dir.path(),
        &[
            "g",
            "-j",
            r#"{"schema":"schema.sql","output":"gen","extractors":["actix_web"]}"#,
        ],
    );
    assert!(stderr.contains(EXTRACTORS_NEED_VALIDATE), "{stderr}");
    let stderr = failure(
        dir.path(),
        &["g", "-s", "schema.sql", "-o", "gen", "--extractor", "axum"],
    );
    assert!(stderr.contains(EXTRACTORS_NEED_VALIDATE), "{stderr}");
    Ok(())
}

/// `serde`, `validate`, and both extractors together run, from the config
/// file, the JSON payload, or flags.
#[test]
fn validate_and_extractors_from_file_payload_or_flags() -> Result<()> {
    let dir = project(Some(
        "derive = [\"serde\", \"validate\"]\nextractors = [\"axum\", \"actix_web\"]\n",
    ))?;
    assert_success(&run_binary_in(dir.path(), &["g"]));

    let dir = project(None)?;
    assert_success(&run_binary_in(
        dir.path(),
        &[
            "g",
            "-j",
            r#"{"schema":"schema.sql","output":"gen","derive":["serde","validate"],"extractors":["axum","actix_web"]}"#,
        ],
    ));
    assert_success(&run_binary_in(
        dir.path(),
        &[
            "g",
            "-s",
            "schema.sql",
            "-o",
            "gen-flags",
            "-d",
            "serde",
            "-d",
            "validate",
            "--extractor",
            "axum",
            "-e",
            "actix_web",
        ],
    ));
    Ok(())
}

/// A higher layer's `extractors` or `derive` list replaces the lower one's
/// whole: flags over the JSON payload over the config file.
#[test]
fn a_higher_layer_replaces_the_lists_whole() -> Result<()> {
    let dir = project(Some(
        "derive = [\"serde\", \"validate\"]\nextractors = [\"axum\"]\n",
    ))?;
    // `-d serde` drops the file's `validate`, leaving its extractors alone.
    let stderr = failure(dir.path(), &["g", "-d", "serde"]);
    assert!(stderr.contains(EXTRACTORS_NEED_VALIDATE), "{stderr}");

    let dir = project(Some("derive = [\"serde\"]\nextractors = [\"axum\"]\n"))?;
    // An empty payload list clears the file's extractors.
    assert_success(&run_binary_in(
        dir.path(),
        &["g", "-j", r#"{"extractors":[]}"#],
    ));

    let dir = project(Some("derive = [\"serde\"]\n"))?;
    // A flag adds extractors the lower layers don't need validate for.
    let stderr = failure(
        dir.path(),
        &["g", "-j", r#"{"extractors":[]}"#, "--extractor", "axum"],
    );
    assert!(stderr.contains(EXTRACTORS_NEED_VALIDATE), "{stderr}");
    assert_success(&run_binary_in(
        dir.path(),
        &[
            "g",
            "-j",
            r#"{"derive":["serde","validate"]}"#,
            "--extractor",
            "axum",
        ],
    ));
    Ok(())
}

/// An unknown extractor or derive name fails, listing the valid ones.
#[test]
fn unknown_names_list_the_valid_ones() -> Result<()> {
    let dir = project(Some("extractors = [\"warp\"]\n"))?;
    let stderr = failure(dir.path(), &["g"]);
    assert!(
        stderr.contains("warp") && stderr.contains("axum") && stderr.contains("actix_web"),
        "{stderr}"
    );

    let dir = project(None)?;
    let stderr = failure(
        dir.path(),
        &[
            "g",
            "-j",
            r#"{"schema":"schema.sql","output":"gen","derive":["serde","validator"]}"#,
        ],
    );
    assert!(
        stderr.contains("validator") && stderr.contains("`validate`"),
        "{stderr}"
    );
    let stderr = failure(
        dir.path(),
        &["g", "-s", "schema.sql", "-o", "gen", "--extractor", "warp"],
    );
    assert!(
        stderr.contains("axum") && stderr.contains("actix_web"),
        "{stderr}"
    );
    Ok(())
}
