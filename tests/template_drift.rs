//! The two API templates share their framework-independent code (decision
//! A6): `templates/actix-api` and `templates/axum-api` hold byte-identical
//! copies of the files [`SHARED`] lists. Each template stays
//! self-contained, so the copies are checked here: an edit made to one
//! template only fails this test, naming the file.
//!
//! To share more, add a path to [`SHARED`]: a directory (every file under
//! it, on both sides) or a file. A feature listed in [`FEATURES`] shares
//! its [`FEATURE_FILES`] that exist in either template; a hand-written
//! feature listed in [`HAND_WRITTEN`], its [`HAND_WRITTEN_FILES`].

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The directories and files both templates hold the same copy of.
const SHARED: &[&str] = &[
    "migrations",
    "queries",
    "src/shared/security",
    // All of `shared/mail`: the sender is started (`tokio::spawn` or
    // actix's runtime) and stopped by each template's `app.rs`.
    "src/shared/mail",
    // The rate limit's counting and `client_ip`; each `rate_limit/mod.rs`
    // is the framework's middleware around it.
    "src/shared/rate_limit/limiter.rs",
    // The `Storage` port and its `object_store` implementation.
    "src/shared/storage",
    // The profile photos' image processing: decode, crop, re-encode.
    "src/shared/thumbnail.rs",
];

/// The features whose framework-independent files both templates share.
const FEATURES: &[&str] = &["users", "audit_log_entry"];

/// The files of a feature no framework shows through: only `routes.rs`
/// and `handlers.rs` (and `mod.rs`, which names them) are rewritten.
const FEATURE_FILES: &[&str] = &[
    "dto.rs",
    "model.rs",
    "repo.rs",
    "service.rs",
    "service_api.rs",
];

/// The hand-written features (the M5 account flows, and uploads) whose [`HAND_WRITTEN_FILES`]
/// both templates share: they have no `model.rs`, `repo.rs` or
/// `service_api.rs`.
const HAND_WRITTEN: &[&str] = &["auth", "account", "media"];

/// The files of a hand-written feature no framework shows through.
const HAND_WRITTEN_FILES: &[&str] = &["dto.rs", "service.rs"];

/// The paths, relative to a template, that [`SHARED`], [`FEATURES`] and
/// [`FEATURE_FILES`] (and [`HAND_WRITTEN`] and
/// [`HAND_WRITTEN_FILES`]) name.
fn shared_paths() -> Vec<String> {
    let mut paths: Vec<String> = SHARED.iter().map(|path| (*path).to_owned()).collect();
    for feature in FEATURES {
        for file in FEATURE_FILES {
            paths.push(format!("src/features/{feature}/{file}"));
        }
    }
    for feature in HAND_WRITTEN {
        for file in HAND_WRITTEN_FILES {
            paths.push(format!("src/features/{feature}/{file}"));
        }
    }
    paths
}

/// Every file under `root/path` (or `root/path` itself, a file), by its
/// path relative to `root` with `/`, and its text with `\n` line endings.
/// None when it doesn't exist.
fn files_under(root: &Path, path: &str) -> Result<BTreeMap<String, String>> {
    fn walk(root: &Path, at: &Path, files: &mut BTreeMap<String, String>) -> Result<()> {
        if at.is_dir() {
            for entry in fs::read_dir(at).with_context(|| format!("reading {}", at.display()))? {
                walk(root, &entry?.path(), files)?;
            }
        } else if at.is_file() {
            let relative = at
                .strip_prefix(root)?
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let text =
                fs::read_to_string(at).with_context(|| format!("reading {}", at.display()))?;
            files.insert(relative, text.replace("\r\n", "\n"));
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    walk(root, &root.join(path), &mut files)?;
    Ok(files)
}

/// Each shared file that differs between the templates at `left` and
/// `right`, or that only one of them has, as a line naming it.
fn drift(left: &Path, right: &Path, paths: &[String]) -> Result<Vec<String>> {
    let (left_name, right_name) = (dir_name(left), dir_name(right));
    let mut found = Vec::new();
    for path in paths {
        let (ours, theirs) = (files_under(left, path)?, files_under(right, path)?);
        let names: std::collections::BTreeSet<&String> = ours.keys().chain(theirs.keys()).collect();
        for name in names {
            match (ours.get(name), theirs.get(name)) {
                (Some(a), Some(b)) if a != b => found.push(format!("{name}: differs")),
                (Some(_), None) => found.push(format!("{name}: only in {left_name}")),
                (None, Some(_)) => found.push(format!("{name}: only in {right_name}")),
                _ => {}
            }
        }
    }
    Ok(found)
}

fn dir_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn templates() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates")
}

#[test]
fn the_templates_share_their_framework_independent_files() -> Result<()> {
    let (actix, axum) = (templates().join("actix-api"), templates().join("axum-api"));
    for path in shared_paths() {
        let exists = actix.join(&path).exists() || axum.join(&path).exists();
        assert!(
            exists,
            "`{path}` is listed as shared but neither template has it"
        );
    }
    let found = drift(&actix, &axum, &shared_paths())?;
    assert!(
        found.is_empty(),
        "the templates' shared files drifted apart (make both copies the same):\n  {}",
        found.join("\n  ")
    );
    Ok(())
}

/// The comparer names a changed file, a file one side lacks, and a file
/// that differs only in line endings as no difference.
#[test]
fn the_comparer_names_each_injected_difference() -> Result<()> {
    let scratch = tempfile::TempDir::new()?;
    let (left, right) = (scratch.path().join("left"), scratch.path().join("right"));
    for root in [&left, &right] {
        fs::create_dir_all(root.join("queries"))?;
        fs::create_dir_all(root.join("src/features/users"))?;
        fs::write(root.join("queries/mail.sql"), "SELECT 1;\n")?;
        fs::write(root.join("src/features/users/dto.rs"), "pub struct Dto;\n")?;
    }
    fs::write(right.join("queries/mail.sql"), "SELECT 1;\r\n")?;
    let paths = ["queries".to_owned(), "src/features/users/dto.rs".to_owned()];
    assert_eq!(drift(&left, &right, &paths)?, Vec::<String>::new());

    fs::write(
        right.join("src/features/users/dto.rs"),
        "pub struct Other;\n",
    )?;
    fs::write(left.join("queries/session.sql"), "SELECT 2;\n")?;
    assert_eq!(
        drift(&left, &right, &paths)?,
        [
            "queries/session.sql: only in left",
            "src/features/users/dto.rs: differs",
        ]
    );
    Ok(())
}
