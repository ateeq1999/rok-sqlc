//! `rok-sqlc init --template <NAME>` copies a ready-made project's files
//! from `templates/`, embedded in the binary, each only when absent. It
//! leaves out the generated ones (`rok-sqlc g` writes them) and generates
//! nothing.

use super::*;

/// The template `actix`'s directory in the repository.
fn actix_template() -> PathBuf {
    common::manifest_dir().join("templates/actix-api")
}

/// Whether `text` is a generated file: `@generated` on its first line.
fn is_generated(text: &str) -> bool {
    text.lines()
        .next()
        .is_some_and(|line| line.contains("@generated"))
}

/// The template's files as `init` copies them: every one but the
/// generated ones and a build's `target/`, with `\n` line endings, and
/// `<path>.template` (its `Cargo.toml`) as `<path>`.
fn copied_files(template: &Path) -> Result<BTreeMap<PathBuf, String>> {
    Ok(read_tree(template)?
        .into_iter()
        .filter(|(path, text)| !path.starts_with("target") && !is_generated(text))
        .map(|(path, text)| {
            let name = path.to_string_lossy().into_owned();
            let path = name.strip_suffix(".template").map_or(path, PathBuf::from);
            (path, text.replace("\r\n", "\n"))
        })
        .collect())
}

#[test]
fn init_template_copies_the_templates_files_but_the_generated_ones() -> Result<()> {
    let dir = TempDir::new()?;
    let project = dir.path().join("app");
    let output = run_binary(&["init", "-t", "actix", project.to_str().unwrap_or_default()]);
    assert_success(&output);

    let expected = copied_files(&actix_template())?;
    assert!(
        expected.contains_key(Path::new("rok-sqlc.toml")),
        "{expected:?}"
    );
    assert!(
        read_tree(&actix_template())?
            .values()
            .any(|text| is_generated(text)),
        "the template holds generated files to leave out"
    );
    assert_eq!(read_tree(&project)?, expected);

    let stdout = String::from_utf8_lossy(&output.stdout);
    for file in expected.keys() {
        assert!(
            stdout.contains(&format!("scaffolded {}", shown(&project.join(file)))),
            "{} not reported:\n{stdout}",
            file.display()
        );
    }
    for step in ["rok-sqlc g", "cargo run"] {
        assert!(
            stdout.contains(step),
            "next step `{step}` missing:\n{stdout}"
        );
    }
    Ok(())
}

#[test]
fn init_template_keeps_existing_files() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(
        dir.path().join("rok-sqlc.toml"),
        "schema = \"schema.sql\"\n",
    )?;
    let output = run_binary_in(dir.path(), &["init", "--template", "actix"]);
    assert_success(&output);
    assert_eq!(
        fs::read_to_string(dir.path().join("rok-sqlc.toml"))?,
        "schema = \"schema.sql\"\n"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("scaffolded rok-sqlc.toml"), "{stdout}");
    assert!(stdout.contains("scaffolded migrations"), "{stdout}");
    Ok(())
}

#[test]
fn init_template_rejects_an_unknown_name_listing_the_valid_ones() -> Result<()> {
    let dir = TempDir::new()?;
    let output = run_binary_in(dir.path(), &["init", "-t", "axum"]);
    assert!(!output.status.success(), "an unknown template must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("axum") && stderr.contains("actix"),
        "{stderr}"
    );
    assert!(read_tree(dir.path())?.is_empty(), "nothing written");
    Ok(())
}
