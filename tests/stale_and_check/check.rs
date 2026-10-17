//! `generate --check`.

use super::*;

#[test]
fn check_passes_on_a_freshly_generated_tree() -> Result<()> {
    let project = Project::new(TWO_TABLES)?;
    project.generate(&[])?;

    for flag in ["--check", "-k"] {
        let output = project.binary(&[flag]);
        assert!(
            output.status.success(),
            "stdout: {}\nstderr: {}",
            stdout(&output),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Runs `--check` and asserts it exits 1, lists `line`, and leaves the tree
/// exactly as it was.
fn assert_check_reports(project: &Project, line: &str) -> Result<()> {
    let before = project.tree()?;
    let output = project.binary(&["--check"]);
    assert_eq!(output.status.code(), Some(1), "stdout: {}", stdout(&output));
    assert!(
        stdout(&output).contains(line),
        "expected `{line}` in:\n{}",
        stdout(&output)
    );
    assert_eq!(project.tree()?, before, "--check modified the filesystem");
    Ok(())
}

#[test]
fn check_reports_a_schema_change() -> Result<()> {
    let project = Project::new(USERS_ONLY)?;
    project.generate(&[])?;
    project
        .set_schema("CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL, name TEXT);")?;

    let models = project.out().join("users").join("models.rs");
    assert_check_reports(&project, &format!("changed {}", shown(&models)))
}

#[test]
fn check_reports_a_hand_edit_to_a_generated_file() -> Result<()> {
    let project = Project::new(USERS_ONLY)?;
    project.generate(&[])?;
    let read = project.out().join("users").join("read.rs");
    let edited = fs::read_to_string(&read)? + "\n// tweak\n";
    fs::write(&read, edited)?;

    assert_check_reports(&project, &format!("changed {}", shown(&read)))
}

#[test]
fn check_reports_a_dropped_table_as_stale() -> Result<()> {
    let project = Project::new(TWO_TABLES)?;
    project.generate(&[])?;
    project.set_schema(USERS_ONLY)?;

    let models = project.out().join("posts").join("models.rs");
    assert_check_reports(&project, &format!("stale {}", shown(&models)))
}

#[test]
fn check_reports_a_new_table_as_missing() -> Result<()> {
    let project = Project::new(USERS_ONLY)?;
    project.generate(&[])?;
    project.set_schema(TWO_TABLES)?;

    let models = project.out().join("posts").join("models.rs");
    assert_check_reports(&project, &format!("missing {}", shown(&models)))
}

#[test]
fn check_on_an_empty_output_directory_writes_nothing() -> Result<()> {
    let project = Project::new(USERS_ONLY)?;
    let output = project.binary(&["--check"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!project.out().exists());
    Ok(())
}

#[test]
fn check_reports_drift_through_the_library() -> Result<()> {
    let project = Project::new(TWO_TABLES)?;
    project.generate(&[])?;
    project.set_schema(USERS_ONLY)?;

    let mut config = rok_sqlc::Config::default();
    config.schema = vec![project.dir.path().join("schema.sql")];
    config.output = project.out();
    config.check = true;
    let err = rok_sqlc::generate(&config).expect_err("drift must fail the check");

    let stale = project.out().join("posts").join("models.rs");
    assert!(err.report().stale.contains(&stale));
    assert!(stale.is_file());
    Ok(())
}
