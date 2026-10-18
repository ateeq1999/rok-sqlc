//! `rok-sqlc init [DIR]` scaffolds the application skeleton of ADR 0016,
//! each file only when absent, and generates nothing.

use super::*;

/// Every file `init` scaffolds, relative to the project directory.
const SKELETON: &[&str] = &[
    "AGENTS.md",
    "rok-sqlc.toml",
    "src/app.rs",
    "src/config.rs",
    "src/error.rs",
    "src/features/mod.rs",
    "src/lib.rs",
    "src/main.rs",
    "src/openapi.rs",
    "src/shared/auth.rs",
    "src/shared/mod.rs",
];

fn skeleton() -> Vec<PathBuf> {
    SKELETON.iter().map(PathBuf::from).collect()
}

#[test]
fn init_scaffolds_the_skeleton_into_the_given_directory() -> Result<()> {
    let dir = TempDir::new()?;
    let project = dir.path().join("app");
    let output = run_binary(&["init", project.to_str().unwrap_or_default()]);
    assert_success(&output);

    let files = read_tree(&project)?;
    assert_eq!(files.keys().cloned().collect::<Vec<_>>(), skeleton());
    assert!(project.join("migrations").is_dir());
    assert!(!project.join("src/db").exists(), "init generates nothing");

    let stdout = String::from_utf8_lossy(&output.stdout);
    for file in SKELETON {
        assert!(
            stdout.contains(&format!("scaffolded {}", shown(&project.join(file)))),
            "{file} not reported:\n{stdout}"
        );
    }
    for step in ["migrations/", "rok-sqlc g", "cargo run"] {
        assert!(
            stdout.contains(step),
            "next step `{step}` missing:\n{stdout}"
        );
    }
    Ok(())
}

#[test]
fn init_defaults_to_the_current_directory() -> Result<()> {
    let dir = TempDir::new()?;
    assert_success(&run_binary_in(dir.path(), &["init"]));
    assert_eq!(
        read_tree(dir.path())?.keys().cloned().collect::<Vec<_>>(),
        skeleton()
    );
    Ok(())
}

#[test]
fn init_rerun_keeps_every_file_as_it_is() -> Result<()> {
    let dir = TempDir::new()?;
    assert_success(&run_binary_in(dir.path(), &["init"]));
    let error = dir.path().join("src/error.rs");
    fs::write(&error, "// my own errors\n")?;
    fs::remove_file(dir.path().join("AGENTS.md"))?;

    let output = run_binary_in(dir.path(), &["init"]);
    assert_success(&output);
    assert_eq!(fs::read_to_string(&error)?, "// my own errors\n");
    assert!(
        dir.path().join("AGENTS.md").is_file(),
        "absent file restored"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("scaffolded AGENTS.md"), "{stdout}");
    assert!(!stdout.contains("scaffolded src/error.rs"), "{stdout}");
    Ok(())
}

#[test]
fn init_keeps_existing_project_files() -> Result<()> {
    let dir = TempDir::new()?;
    fs::create_dir_all(dir.path().join("src"))?;
    fs::write(dir.path().join("src/main.rs"), "fn main() {}\n")?;
    fs::write(
        dir.path().join("rok-sqlc.toml"),
        "schema = \"schema.sql\"\n",
    )?;

    assert_success(&run_binary_in(dir.path(), &["init"]));
    assert_eq!(
        fs::read_to_string(dir.path().join("src/main.rs"))?,
        "fn main() {}\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("rok-sqlc.toml"))?,
        "schema = \"schema.sql\"\n"
    );
    assert!(dir.path().join("src/app.rs").is_file());
    Ok(())
}

/// `main.rs` starts the application through its library, named after the
/// package, or after the directory when there is no `Cargo.toml`.
#[test]
fn init_names_the_library_main_calls_into() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"my-api\"\nversion = \"0.1.0\"\n",
    )?;
    assert_success(&run_binary_in(dir.path(), &["init"]));
    let main = fs::read_to_string(dir.path().join("src/main.rs"))?;
    assert!(main.contains("\nuse my_api::{app, config};\n"), "{main}");

    let bare = dir.path().join("Shop-Api");
    assert_success(&run_binary(&["init", bare.to_str().unwrap_or_default()]));
    let main = fs::read_to_string(bare.join("src/main.rs"))?;
    assert!(main.contains("\nuse shop_api::{app, config};\n"), "{main}");
    Ok(())
}

/// Cargo's untouched default `main.rs` (`cargo new`) is not the user's
/// code: `init` replaces it with the scaffolded one and says so. Any other
/// `main.rs` is kept.
#[test]
fn init_replaces_cargos_default_main_and_keeps_any_other() -> Result<()> {
    for default in [
        "fn main() {\n    println!(\"Hello, world!\");\n}\n",
        "fn main() {\r\n    println!(\"Hello, world!\");\r\n}\r\n",
        "fn main() {\n    println!(\"Hello, world!\");\n}",
    ] {
        let dir = TempDir::new()?;
        fs::create_dir_all(dir.path().join("src"))?;
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n",
        )?;
        fs::write(dir.path().join("src/main.rs"), default)?;
        let output = run_binary_in(dir.path(), &["init"]);
        assert_success(&output);
        let main = fs::read_to_string(dir.path().join("src/main.rs"))?;
        assert!(main.contains("\nuse shop::{app, config};\n"), "{main}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("replaced cargo's default src/main.rs"),
            "{stdout}"
        );
    }

    let dir = TempDir::new()?;
    fs::create_dir_all(dir.path().join("src"))?;
    let mine = "fn main() {\n    println!(\"Hello, shop!\");\n}\n";
    fs::write(dir.path().join("src/main.rs"), mine)?;
    let output = run_binary_in(dir.path(), &["init"]);
    assert_success(&output);
    assert_eq!(fs::read_to_string(dir.path().join("src/main.rs"))?, mine);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("replaced"), "{stdout}");
    Ok(())
}
