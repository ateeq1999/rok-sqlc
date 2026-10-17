//! Stale-file removal and `generate --check`, driven through the CLI.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::Result;
use tempfile::TempDir;

mod common;

use common::shown;

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
        read_tree(&self.out())
    }
}

/// Every file (with its contents) and directory under `root`, by relative path.
fn read_tree(root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, String>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let rel = path.strip_prefix(root)?.to_path_buf();
            if path.is_dir() {
                files.insert(rel, "<dir>".to_owned());
                walk(root, &path, files)?;
            } else {
                files.insert(rel, fs::read_to_string(&path)?);
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files)?;
    Ok(files)
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
        stdout(&dry).contains(&format!("would remove {}", shown(&posts_models))),
        "{}",
        stdout(&dry)
    );
    assert!(posts_models.is_file());

    let real = project.binary(&[]);
    assert!(real.status.success());
    assert!(stdout(&real).contains(&format!("removed {}", shown(&posts_models))));
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

/// Writes `queries/users.sql` and `queries/emails.sql` into the project and
/// returns the directory.
fn write_query_dir(project: &Project) -> Result<PathBuf> {
    let dir = project.dir.path().join("queries");
    fs::create_dir_all(&dir)?;
    fs::write(
        dir.join("users.sql"),
        "-- name: GetUserById :maybe_one\nSELECT id, email FROM users WHERE id = $1;\n",
    )?;
    fs::write(
        dir.join("emails.sql"),
        "-- name: GetUserByEmail :maybe_one\nSELECT id, email FROM users WHERE email = $1;\n",
    )?;
    Ok(dir)
}

#[test]
fn a_kept_queries_directory_is_not_stale() -> Result<()> {
    let project = Project::new(USERS_ONLY)?;
    let dir = write_query_dir(&project)?;
    project.generate(&["--queries", dir.to_str().unwrap()])?;
    let before = project.tree()?;

    project.generate(&[])?;

    assert_eq!(project.tree()?, before);
    Ok(())
}

#[test]
fn switching_from_a_query_file_to_a_directory_removes_queries_rs() -> Result<()> {
    let project = Project::new(USERS_ONLY)?;
    let file = project.dir.path().join("queries.sql");
    fs::write(
        &file,
        "-- name: GetUserByEmail :maybe_one\nSELECT id, email FROM users WHERE email = $1;\n",
    )?;
    project.generate(&["--queries", file.to_str().unwrap()])?;
    assert!(project.out().join("queries.rs").is_file());

    let dir = write_query_dir(&project)?;
    let output = project.binary(&["--queries", dir.to_str().unwrap()]);
    assert!(output.status.success(), "{output:?}");

    let out = project.out();
    assert!(!out.join("queries.rs").exists());
    assert!(out.join("queries/mod.rs").is_file());
    assert!(out.join("queries/users.rs").is_file());
    assert!(out.join("queries/emails.rs").is_file());
    let removed = out.join("queries.rs");
    assert!(
        stdout(&output).contains(&format!("removed {}", shown(&removed))),
        "{}",
        stdout(&output)
    );
    Ok(())
}

#[test]
fn switching_from_a_directory_to_a_query_file_removes_the_queries_directory() -> Result<()> {
    let project = Project::new(USERS_ONLY)?;
    let dir = write_query_dir(&project)?;
    project.generate(&["--queries", dir.to_str().unwrap()])?;
    assert!(project.out().join("queries/mod.rs").is_file());

    let file = dir.join("users.sql");
    project.generate(&["--queries", file.to_str().unwrap()])?;

    let out = project.out();
    assert!(out.join("queries.rs").is_file());
    assert!(!out.join("queries").exists());
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

/// Generates from a migrations directory holding `files` (name, SQL) into
/// `<dir>/out`.
fn generate_migrations(dir: &Path, files: &[(&str, &str)]) -> Result<()> {
    let migrations = dir.join("migrations");
    fs::create_dir_all(&migrations)?;
    for (name, sql) in files {
        fs::write(migrations.join(name), sql)?;
    }
    rok_sqlc::cli::run_from([
        "rok-sqlc".to_owned(),
        "g".to_owned(),
        "-s".to_owned(),
        migrations.display().to_string(),
        "-o".to_owned(),
        dir.join("out").display().to_string(),
    ])
}

const BLOG: &str = "CREATE TABLE app.posts (id BIGSERIAL PRIMARY KEY, title TEXT NOT NULL);\n\
     CREATE TABLE comments (\n\
         id BIGSERIAL PRIMARY KEY,\n\
         post_id BIGINT NOT NULL REFERENCES app.posts (id),\n\
         body TEXT NOT NULL\n\
     );\n";

#[test]
fn a_table_dropped_by_a_later_migration_is_removed_with_its_files() -> Result<()> {
    let dir = TempDir::new()?;
    generate_migrations(dir.path(), &[("0001_init.up.sql", BLOG)])?;
    let out = dir.path().join("out");
    assert!(out.join("posts/models.rs").is_file());

    generate_migrations(
        dir.path(),
        &[(
            "0002_drop_posts.up.sql",
            "DROP TABLE IF EXISTS app.posts CASCADE;\nDROP TABLE IF EXISTS never_existed;\n",
        )],
    )?;

    assert!(!out.join("posts").exists(), "posts was not removed");
    let db_mod = fs::read_to_string(out.join("mod.rs"))?;
    assert!(!db_mod.contains("pub mod posts;"), "{db_mod}");
    // The foreign key went with the table (as with CASCADE): the column keeps
    // its raw type and no relationship points at the dropped table.
    let comments = fs::read_to_string(out.join("comments/models.rs"))?;
    assert!(comments.contains("pub post_id: i64,"), "{comments}");
    assert!(!out.join("comments/relationships.rs").exists());
    Ok(())
}

#[test]
fn a_table_can_be_recreated_after_it_was_dropped() -> Result<()> {
    let dir = TempDir::new()?;
    generate_migrations(
        dir.path(),
        &[
            ("0001_init.up.sql", BLOG),
            ("0002_drop.up.sql", "DROP TABLE comments;"),
            (
                "0003_recreate.up.sql",
                "CREATE TABLE comments (id BIGSERIAL PRIMARY KEY, text TEXT NOT NULL);",
            ),
        ],
    )?;
    let comments = fs::read_to_string(dir.path().join("out/comments/models.rs"))?;
    assert!(comments.contains("pub text: String,"), "{comments}");
    assert!(!comments.contains("post_id"), "{comments}");
    Ok(())
}

// --- line endings -------------------------------------------------------------

const QUERIES: &str = "-- name: UserEmails :many\nSELECT email\nFROM users\nORDER BY email;\n";

fn crlf(text: &str) -> String {
    text.replace('\n', "\r\n")
}

/// Generates `schema` and `queries` into a fresh project and returns the
/// generated tree.
fn generate_tree(schema: &str, queries: &str) -> Result<(Project, BTreeMap<PathBuf, String>)> {
    let project = Project::new(schema)?;
    let queries_path = project.dir.path().join("queries.sql");
    fs::write(&queries_path, queries)?;
    project.generate(&["--queries", &queries_path.display().to_string()])?;
    let tree = project.tree()?;
    Ok((project, tree))
}

#[test]
fn crlf_inputs_generate_the_same_files_as_lf_inputs() -> Result<()> {
    let schema =
        "-- Users.\nCREATE TABLE users (\n    id UUID PRIMARY KEY,\n    email TEXT NOT NULL\n);\n";
    let (_, lf) = generate_tree(schema, QUERIES)?;
    let (_, from_crlf) = generate_tree(&crlf(schema), &crlf(QUERIES))?;
    assert_eq!(from_crlf, lf);
    assert!(!lf[Path::new("schema.rs")].contains('\r'));
    Ok(())
}

#[test]
fn check_ignores_line_endings_of_generated_files() -> Result<()> {
    let (project, tree) = generate_tree(USERS_ONLY, QUERIES)?;
    // As Git checks files out with `core.autocrlf = true`.
    for (path, contents) in &tree {
        if contents != "<dir>" {
            fs::write(project.out().join(path), crlf(contents))?;
        }
    }
    let queries = project.dir.path().join("queries.sql");
    fs::write(&queries, crlf(QUERIES))?;
    project.set_schema(&crlf(USERS_ONLY))?;

    let queries = queries.display().to_string();
    let check = project.binary(&["--check", "--queries", &queries]);
    assert!(check.status.success(), "{}", stdout(&check));

    let run = project.binary(&["--queries", &queries]);
    assert!(run.status.success());
    assert!(!stdout(&run).contains("generated "), "{}", stdout(&run));
    Ok(())
}

// --- lenient runs keep skipped tables' files ---------------------------------

const BREAK_POSTS: &str = "ALTER TABLE posts ADD COLUMN duration INTERVAL;\n";

/// A migrations directory and an output directory in a temp dir, driven
/// through the real binary.
struct Migrations {
    dir: TempDir,
}

impl Migrations {
    fn new(files: &[(&str, &str)]) -> Result<Self> {
        let migrations = Self {
            dir: TempDir::new()?,
        };
        migrations.add(files)?;
        Ok(migrations)
    }

    fn add(&self, files: &[(&str, &str)]) -> Result<()> {
        let dir = self.dir.path().join("migrations");
        fs::create_dir_all(&dir)?;
        for (name, sql) in files {
            fs::write(dir.join(name), sql)?;
        }
        Ok(())
    }

    fn out(&self) -> PathBuf {
        self.dir.path().join("out")
    }

    fn run(&self, extra: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
            .arg("generate")
            .arg("--schema")
            .arg(self.dir.path().join("migrations"))
            .arg("--output")
            .arg(self.out())
            .args(extra)
            .output()
            .expect("failed to spawn rok-sqlc binary")
    }

    fn tree(&self) -> Result<BTreeMap<PathBuf, String>> {
        read_tree(&self.out())
    }

    fn resource(&self, module: &str) -> Result<BTreeMap<PathBuf, String>> {
        Ok(self
            .tree()?
            .into_iter()
            .filter(|(path, _)| path.starts_with(module))
            .collect())
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_table_skipped_by_lenient_keeps_its_files() -> Result<()> {
    let project = Migrations::new(&[("0001_init.up.sql", TWO_TABLES)])?;
    assert!(project.run(&[]).status.success());
    let before = project.resource("posts")?;
    assert!(before.contains_key(Path::new("posts").join("models.rs").as_path()));

    project.add(&[("0002_break.up.sql", BREAK_POSTS)])?;
    let output = project.run(&["--lenient"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(
        stderr(&output).contains("table `posts` skipped"),
        "{}",
        stderr(&output)
    );
    assert_eq!(project.resource("posts")?, before);
    let db_mod = fs::read_to_string(project.out().join("mod.rs"))?;
    assert!(db_mod.contains("pub mod posts;"), "{db_mod}");
    assert!(db_mod.contains("pub mod users;"), "{db_mod}");
    Ok(())
}

#[test]
fn a_lenient_check_reports_the_failure_but_no_drift_for_kept_files() -> Result<()> {
    let project = Migrations::new(&[("0001_init.up.sql", TWO_TABLES)])?;
    assert!(project.run(&[]).status.success());
    project.add(&[("0002_break.up.sql", BREAK_POSTS)])?;
    assert!(project.run(&["--lenient"]).status.success());
    let before = project.tree()?;

    let output = project.run(&["--lenient", "--check"]);

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    assert!(stderr(&output).contains("table `posts` skipped"));
    assert!(!stdout(&output).contains("stale"), "{}", stdout(&output));
    assert_eq!(project.tree()?, before);
    Ok(())
}

#[test]
fn a_dropped_table_is_still_removed_in_a_lenient_run() -> Result<()> {
    let project = Migrations::new(&[("0001_init.up.sql", TWO_TABLES)])?;
    assert!(project.run(&[]).status.success());
    project.add(&[
        ("0002_break.up.sql", BREAK_POSTS),
        ("0003_drop.up.sql", "DROP TABLE posts;\n"),
    ])?;

    let output = project.run(&["--lenient"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(!project.out().join("posts").exists());
    let db_mod = fs::read_to_string(project.out().join("mod.rs"))?;
    assert!(!db_mod.contains("pub mod posts;"), "{db_mod}");
    Ok(())
}

#[test]
fn a_strict_run_that_hits_the_failure_writes_nothing() -> Result<()> {
    let project = Migrations::new(&[("0001_init.up.sql", TWO_TABLES)])?;
    assert!(project.run(&[]).status.success());
    let before = project.tree()?;
    project.add(&[("0002_break.up.sql", BREAK_POSTS)])?;

    let output = project.run(&[]);

    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    assert!(stderr(&output).contains("posts"), "{}", stderr(&output));
    assert_eq!(project.tree()?, before);
    Ok(())
}

#[test]
fn a_kept_versioned_table_keeps_update_outcome_in_mod_rs() -> Result<()> {
    let project = Migrations::new(&[(
        "0001_init.up.sql",
        "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);\n\
         CREATE TABLE posts (\n\
             id BIGSERIAL PRIMARY KEY,\n\
             title TEXT NOT NULL,\n\
             row_version BIGINT NOT NULL DEFAULT 1\n\
         );\n",
    )])?;
    let version = r#"{"conventions": {"version_column": "row_version"}}"#;
    assert!(project.run(&["--json", version]).status.success());
    let before = fs::read_to_string(project.out().join("mod.rs"))?;
    assert!(before.contains("pub enum UpdateOutcome"), "{before}");

    project.add(&[("0002_break.up.sql", BREAK_POSTS)])?;
    let output = project.run(&["--lenient", "--json", version]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let after = fs::read_to_string(project.out().join("mod.rs"))?;
    assert!(after.contains("pub enum UpdateOutcome"), "{after}");
    Ok(())
}
