//! The stable library entry point, `rok_sqlc::generate`, called the way a
//! `build.rs` would call it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use rok_sqlc::Config;
use tempfile::TempDir;

const SCHEMA: &str = "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);\n\
     CREATE TABLE spans (id BIGSERIAL PRIMARY KEY, duration INTERVAL NOT NULL);";

const QUERIES: &str = "-- name: GetUserByEmail :maybe_one
SELECT id, email FROM users WHERE email = $1;

-- name: Broken :one
SELECT id FROM no_such_table WHERE id = $1;
";

struct Inputs {
    dir: TempDir,
}

impl Inputs {
    fn new() -> Result<Self> {
        let dir = TempDir::new()?;
        fs::write(dir.path().join("schema.sql"), SCHEMA)?;
        fs::write(dir.path().join("queries.sql"), QUERIES)?;
        Ok(Self { dir })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn config(&self, output: &str) -> Config {
        let mut config = Config::default();
        config.schema = vec![self.path("schema.sql")];
        config.queries = vec![self.path("queries.sql")];
        config.output = self.path(output);
        config.lenient = true;
        config
    }
}

/// Every file under `root`, keyed by its path relative to `root`.
fn tree(root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, String>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                walk(root, &path, out)?;
            } else {
                out.insert(
                    path.strip_prefix(root)?.to_path_buf(),
                    fs::read_to_string(&path)?,
                );
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

#[test]
fn generate_writes_the_same_files_as_the_cli() -> Result<()> {
    let inputs = Inputs::new()?;

    rok_sqlc::cli::run_from([
        "rok-sqlc",
        "generate",
        "--schema",
        inputs.path("schema.sql").to_str().unwrap(),
        "--queries",
        inputs.path("queries.sql").to_str().unwrap(),
        "--output",
        inputs.path("cli").to_str().unwrap(),
        "--lenient",
    ])?;
    rok_sqlc::generate(&inputs.config("lib"))?;

    let cli = tree(&inputs.path("cli"))?;
    let lib = tree(&inputs.path("lib"))?;
    assert!(cli.contains_key(Path::new("users/models.rs")));
    assert!(cli.contains_key(Path::new("queries.rs")));
    assert_eq!(cli, lib);
    Ok(())
}

#[test]
fn report_lists_written_then_unchanged_files() -> Result<()> {
    let inputs = Inputs::new()?;
    let config = inputs.config("out");

    let first = rok_sqlc::generate(&config)?;
    assert!(first.written.contains(&inputs.path("out/users/models.rs")));
    assert!(first.unchanged.is_empty());

    let second = rok_sqlc::generate(&config)?;
    assert!(second.written.is_empty());
    assert!(
        second
            .unchanged
            .contains(&inputs.path("out/users/models.rs"))
    );
    Ok(())
}

#[test]
fn lenient_report_lists_skipped_tables_and_queries() -> Result<()> {
    let inputs = Inputs::new()?;

    let report = rok_sqlc::generate(&inputs.config("out"))?;

    let tables: Vec<_> = report
        .table_failures
        .iter()
        .map(|f| f.table.as_str())
        .collect();
    assert_eq!(tables, ["spans"]);
    let queries: Vec<_> = report
        .query_failures
        .iter()
        .map(|f| f.query.as_str())
        .collect();
    assert_eq!(queries, ["Broken"]);
    assert!(!inputs.path("out/spans").exists());
    Ok(())
}

#[test]
fn strict_failure_is_an_error_whose_report_names_the_table() -> Result<()> {
    let inputs = Inputs::new()?;
    let mut config = inputs.config("out");
    config.lenient = false;

    let error = rok_sqlc::generate(&config).unwrap_err();

    let tables: Vec<_> = error
        .report()
        .table_failures
        .iter()
        .map(|f| f.table.as_str())
        .collect();
    assert_eq!(tables, ["spans"]);
    assert!(error.to_string().contains("1 unsupported table"));
    assert!(
        !inputs.path("out").exists(),
        "a strict failure writes nothing"
    );
    Ok(())
}

#[test]
fn refused_files_are_an_error_whose_report_lists_them() -> Result<()> {
    let inputs = Inputs::new()?;
    let config = inputs.config("out");
    let guarded = inputs.path("out/users/models.rs");
    fs::create_dir_all(guarded.parent().unwrap())?;
    fs::write(&guarded, "// hand-written, precious\n")?;

    let error = rok_sqlc::generate(&config).unwrap_err();

    let refused: Vec<_> = error.report().refused.iter().map(|r| &r.path).collect();
    assert_eq!(refused, [&guarded]);
    assert!(error.report().written.is_empty(), "nothing was written");
    assert_eq!(fs::read_to_string(&guarded)?, "// hand-written, precious\n");
    assert!(!inputs.path("out/mod.rs").exists());
    Ok(())
}

#[test]
fn dry_run_reports_refusals_without_failing_or_writing() -> Result<()> {
    let inputs = Inputs::new()?;
    let mut config = inputs.config("out");
    config.dry_run = true;
    let guarded = inputs.path("out/users/models.rs");
    fs::create_dir_all(guarded.parent().unwrap())?;
    fs::write(&guarded, "// hand-written, precious\n")?;

    let report = rok_sqlc::generate(&config)?;

    assert_eq!(report.refused.len(), 1);
    assert!(report.written.contains(&inputs.path("out/mod.rs")));
    assert!(!inputs.path("out/mod.rs").exists());
    Ok(())
}

#[test]
fn schema_inputs_are_combined_in_order() -> Result<()> {
    let inputs = Inputs::new()?;
    fs::write(
        inputs.path("first.sql"),
        "CREATE TABLE users (id UUID PRIMARY KEY);",
    )?;
    fs::write(
        inputs.path("second.sql"),
        "CREATE TABLE posts (id UUID PRIMARY KEY, author_id UUID NOT NULL REFERENCES users (id));",
    )?;
    let mut config = Config::default();
    config.schema = vec![inputs.path("first.sql"), inputs.path("second.sql")];
    config.output = inputs.path("out");

    rok_sqlc::generate(&config)?;

    assert!(inputs.path("out/users/models.rs").exists());
    assert!(inputs.path("out/posts/models.rs").exists());
    Ok(())
}

#[test]
fn report_failures_carry_their_source_path_and_line() -> Result<()> {
    let inputs = Inputs::new()?;

    let report = rok_sqlc::generate(&inputs.config("out"))?;

    let table = &report.table_failures[0];
    assert_eq!(
        table.path.as_deref(),
        Some(inputs.path("schema.sql").as_path())
    );
    assert_eq!(table.line, Some(2));
    let query = &report.query_failures[0];
    assert_eq!(
        query.path.as_deref(),
        Some(inputs.path("queries.sql").as_path())
    );
    assert_eq!(query.line, Some(4));
    Ok(())
}

/// Without `derive`, nothing generated mentions `serde`.
#[test]
fn generated_code_mentions_serde_only_when_derived() -> Result<()> {
    let inputs = Inputs::new()?;
    rok_sqlc::generate(&inputs.config("plain"))?;
    for (path, text) in tree(&inputs.path("plain"))? {
        assert!(!text.contains("serde"), "{} mentions serde", path.display());
    }

    let mut config = inputs.config("serde");
    config.derive = vec![rok_sqlc::Derive::Serde];
    rok_sqlc::generate(&config)?;
    let models = fs::read_to_string(inputs.path("serde/users/models.rs"))?;
    let serde = "serde::Serialize,";
    // Each item's attributes (between its doc comment and the item) derive
    // serde; a typed id serializes as its inner value.
    let attributes = |item: &str| {
        let before = models.split(&format!("pub struct {item}")).next().unwrap();
        let doc_end = before.rfind("///").unwrap();
        before[doc_end..].to_owned()
    };
    assert!(attributes("UserId(").contains(serde), "{models}");
    assert!(
        attributes("UserId(").contains("#[serde(transparent)]\n"),
        "{models}"
    );
    for item in ["User ", "NewUser ", "UpdateUser "] {
        assert!(attributes(item).contains(serde), "{item}:\n{models}");
    }
    let hints = fs::read_to_string(inputs.path("serde/mod.rs"))?;
    assert!(
        hints.contains("//   serde = { version = \"1\", features = [\"derive\"] }\n"),
        "{hints}"
    );
    assert!(
        hints.contains("//   uuid = { version = \"1\", features = [\"serde\"] }\n"),
        "{hints}"
    );
    Ok(())
}
