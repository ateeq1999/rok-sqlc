//! `rok-sqlc init --template <NAME>` copies a ready-made project's files
//! from `templates/`, embedded in the binary, each only when absent. It
//! leaves out rok-sqlc's generated ones (`rok-sqlc g` writes them), but
//! copies the template's pinned `Cargo.lock`, and generates nothing.

use super::*;

/// The template `actix`'s directory in the repository.
fn actix_template() -> PathBuf {
    common::manifest_dir().join("templates/actix-api")
}

/// The template's files as `init` copies them: every one but
/// rok-sqlc's generated ones and a build's `target/`, with `\n` line endings, and
/// `<path>.template` (its `Cargo.toml`) as `<path>`.
fn copied_files(template: &Path) -> Result<BTreeMap<PathBuf, String>> {
    Ok(read_tree(template)?
        .into_iter()
        .filter(|(path, text)| !path.starts_with("target") && !common::is_generated(text))
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
        expected.contains_key(Path::new("rok-sqlc.toml"))
            && expected.contains_key(Path::new("Cargo.lock")),
        "{expected:?}"
    );
    assert!(
        read_tree(&actix_template())?
            .values()
            .any(|text| common::is_generated(text)),
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

/// The step lines after `Next steps:`.
fn next_steps(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .skip_while(|line| *line != "Next steps:")
        .skip(1)
        .collect()
}

#[test]
fn init_template_prints_numbered_next_steps_without_the_dependencies_it_copied() -> Result<()> {
    let dir = TempDir::new()?;
    let output = run_binary_in(dir.path(), &["init", "-t", "actix"]);
    assert_success(&output);
    assert!(dir.path().join("Cargo.toml").is_file());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let steps = next_steps(&stdout);
    assert_eq!(
        steps,
        [
            "  1. run `rok-sqlc g` to generate src/db/, src/shared/ and src/features/ from migrations/",
            "  2. set DATABASE_URL and JWT_SECRET (see .env.example) and apply migrations/",
            "  3. `cargo test`, then `cargo run`",
        ],
        "{stdout}"
    );
    Ok(())
}

#[test]
fn init_template_into_a_package_asks_for_the_templates_dependencies() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"shop\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    let output = run_binary_in(dir.path(), &["init", "-t", "actix"]);
    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let steps = next_steps(&stdout);
    assert_eq!(
        steps.first().copied(),
        Some(
            "  1. add the template's dependencies and lints (its README.md lists them) to Cargo.toml"
        ),
        "{stdout}"
    );
    assert_eq!(steps.len(), 4, "{stdout}");
    Ok(())
}
