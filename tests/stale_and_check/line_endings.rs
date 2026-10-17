//! Line endings of inputs and of generated files.

use super::*;

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
