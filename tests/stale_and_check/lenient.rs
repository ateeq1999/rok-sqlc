//! Lenient runs keep the files of the tables they skip.

use super::*;

const BREAK_POSTS: &str = "ALTER TABLE posts ADD COLUMN duration INET;\n";

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

/// The kept directory is the module the table generates under, which a
/// `[modules]` rename sets.
#[test]
fn a_renamed_table_skipped_by_lenient_keeps_its_files() -> Result<()> {
    let project = Migrations::new(&[("0001_init.up.sql", TWO_TABLES)])?;
    let rename = r#"{"modules": {"posts": "articles"}}"#;
    assert!(project.run(&["--json", rename]).status.success());
    let before = project.resource("articles")?;
    assert!(before.contains_key(Path::new("articles").join("models.rs").as_path()));

    project.add(&[("0002_break.up.sql", BREAK_POSTS)])?;
    let output = project.run(&["--lenient", "--json", rename]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(project.resource("articles")?, before);
    let db_mod = fs::read_to_string(project.out().join("mod.rs"))?;
    assert!(db_mod.contains("pub mod articles;"), "{db_mod}");
    Ok(())
}

/// A table that fails at its own `CREATE TABLE` has no last good shape to
/// plan the other files against, so its earlier files are stale: keeping
/// them could leave the generated code not compiling.
#[test]
fn a_table_failing_at_its_create_table_is_not_kept() -> Result<()> {
    let project = Migrations::new(&[("0001_init.up.sql", TWO_TABLES)])?;
    assert!(project.run(&[]).status.success());
    assert!(project.out().join("posts/models.rs").is_file());

    project.add(&[(
        "0001_init.up.sql",
        "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);\n\
         CREATE TABLE posts (id BIGSERIAL PRIMARY KEY, title TEXT NOT NULL, origin INET);",
    )])?;
    let output = project.run(&["--lenient"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(stderr(&output).contains("table `posts` skipped"));
    assert!(!project.out().join("posts").exists());
    let db_mod = fs::read_to_string(project.out().join("mod.rs"))?;
    assert!(!db_mod.contains("pub mod posts;"), "{db_mod}");
    Ok(())
}
