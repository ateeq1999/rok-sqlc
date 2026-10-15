//! Stale-file removal and `generate --check`, driven through the CLI.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::Result;
use tempfile::TempDir;

const TWO_TABLES: &str = "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);\n\
     CREATE TABLE posts (id BIGSERIAL PRIMARY KEY, title TEXT NOT NULL);";
const USERS_ONLY: &str = "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);";

/// A schema file and an output directory in a temp dir.
struct Project {
    dir: TempDir,
}

impl Project {
    fn new(schema: &str) -> Result<Self> {
        let dir = TempDir::new()?;
        fs::write(dir.path().join("schema.sql"), schema)?;
        Ok(Self { dir })
    }

    fn set_schema(&self, schema: &str) -> Result<()> {
        fs::write(self.dir.path().join("schema.sql"), schema)?;
        Ok(())
    }

    fn out(&self) -> PathBuf {
        self.dir.path().join("out")
    }

    fn args<'a>(&'a self, extra: &[&'a str]) -> Vec<String> {
        let mut args = vec![
            "generate".to_owned(),
            "--schema".to_owned(),
            self.dir.path().join("schema.sql").display().to_string(),
            "--output".to_owned(),
            self.out().display().to_string(),
        ];
        args.extend(extra.iter().map(|arg| (*arg).to_owned()));
        args
    }

    fn generate(&self, extra: &[&str]) -> Result<()> {
        let mut full = vec!["rok-sqlc".to_owned()];
        full.extend(self.args(extra));
        rok_sqlc::cli::run_from(full)
    }

    fn binary(&self, extra: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
            .args(self.args(extra))
            .output()
            .expect("failed to spawn rok-sqlc binary")
    }

    fn tree(&self) -> Result<BTreeMap<PathBuf, String>> {
        fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, String>) -> Result<()> {
            for entry in fs::read_dir(dir)? {
                let path = entry?.path();
                if path.is_dir() {
                    files.insert(path.strip_prefix(root)?.to_path_buf(), "<dir>".to_owned());
                    walk(root, &path, files)?;
                } else {
                    let rel = path.strip_prefix(root)?.to_path_buf();
                    files.insert(rel, fs::read_to_string(&path)?);
                }
            }
            Ok(())
        }
        let mut files = BTreeMap::new();
        walk(&self.out(), &self.out(), &mut files)?;
        Ok(files)
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn regenerating_after_a_dropped_table_removes_its_resource_directory() -> Result<()> {
    let project = Project::new(TWO_TABLES)?;
    project.generate(&[])?;
    assert!(project.out().join("posts/models.rs").is_file());

    project.set_schema(USERS_ONLY)?;
    project.generate(&[])?;

    assert!(!project.out().join("posts").exists());
    assert!(project.out().join("users/models.rs").is_file());
    let db_mod = fs::read_to_string(project.out().join("mod.rs"))?;
    assert!(!db_mod.contains("pub mod posts;"));
    Ok(())
}

#[test]
fn removal_is_reported_and_dry_run_only_lists_it() -> Result<()> {
    let project = Project::new(TWO_TABLES)?;
    project.generate(&[])?;
    project.set_schema(USERS_ONLY)?;

    let dry = project.binary(&["--dry-run"]);
    assert!(dry.status.success());
    let posts_models = project.out().join("posts").join("models.rs");
    assert!(
        stdout(&dry).contains(&format!("would remove {}", posts_models.display())),
        "{}",
        stdout(&dry)
    );
    assert!(posts_models.is_file());

    let real = project.binary(&[]);
    assert!(real.status.success());
    assert!(stdout(&real).contains(&format!("removed {}", posts_models.display())));
    assert!(!posts_models.exists());
    Ok(())
}

#[test]
fn hand_written_files_survive_regeneration() -> Result<()> {
    let project = Project::new(TWO_TABLES)?;
    project.generate(&[])?;
    let notes = project.out().join("notes.rs");
    let helper = project.out().join("posts").join("helpers.rs");
    fs::write(&notes, "// hand-written\n")?;
    fs::write(&helper, "// hand-written helper\n")?;

    project.set_schema(USERS_ONLY)?;
    project.generate(&[])?;

    assert_eq!(fs::read_to_string(&notes)?, "// hand-written\n");
    assert_eq!(fs::read_to_string(&helper)?, "// hand-written helper\n");
    assert!(!project.out().join("posts/models.rs").exists());
    Ok(())
}

#[test]
fn filtered_runs_keep_other_generated_resources() -> Result<()> {
    let project = Project::new(TWO_TABLES)?;
    project.generate(&[])?;
    let before = project.tree()?;

    project.generate(&["--resource", "users"])?;

    assert_eq!(project.tree()?, before);
    Ok(())
}

#[test]
fn a_kept_queries_module_is_not_stale() -> Result<()> {
    let project = Project::new(USERS_ONLY)?;
    let queries = project.dir.path().join("queries.sql");
    fs::write(
        &queries,
        "-- name: GetUserByEmail :maybe_one\nSELECT id, email FROM users WHERE email = $1;\n",
    )?;
    project.generate(&["--queries", queries.to_str().unwrap()])?;

    project.generate(&[])?;

    assert!(project.out().join("queries.rs").is_file());
    Ok(())
}

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
    assert_check_reports(&project, &format!("changed {}", models.display()))
}

#[test]
fn check_reports_a_hand_edit_to_a_generated_file() -> Result<()> {
    let project = Project::new(USERS_ONLY)?;
    project.generate(&[])?;
    let read = project.out().join("users").join("read.rs");
    let edited = fs::read_to_string(&read)? + "\n// tweak\n";
    fs::write(&read, edited)?;

    assert_check_reports(&project, &format!("changed {}", read.display()))
}

#[test]
fn check_reports_a_dropped_table_as_stale() -> Result<()> {
    let project = Project::new(TWO_TABLES)?;
    project.generate(&[])?;
    project.set_schema(USERS_ONLY)?;

    let models = project.out().join("posts").join("models.rs");
    assert_check_reports(&project, &format!("stale {}", models.display()))
}

#[test]
fn check_reports_a_new_table_as_missing() -> Result<()> {
    let project = Project::new(USERS_ONLY)?;
    project.generate(&[])?;
    project.set_schema(TWO_TABLES)?;

    let models = project.out().join("posts").join("models.rs");
    assert_check_reports(&project, &format!("missing {}", models.display()))
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
