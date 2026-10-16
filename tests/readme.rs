//! Keeps README.md honest: its flags and config keys match the code, and its
//! code samples match the quickstart example in `docs/examples/quickstart`,
//! which the current binary generates and which compiles.
//!
//! README conventions these tests read:
//!
//! - A fenced block right after `<!-- source: <path> -->` must appear
//!   verbatim in that file (repository-relative). Every `rust` block needs
//!   one, so no Rust sample is hand-written.
//! - A fenced block right after `<!-- tree: <dir> -->` must be the file tree
//!   of that directory.
//! - The `toml` block after `<!-- config-reference -->` lists every config
//!   key, and the table rows starting with ``| `-`` list every flag.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, bail, ensure};
use tempfile::TempDir;

mod common;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The example crates the README's samples come from, each generated from
/// its own SQL by `rok-sqlc g` with its `rok-sqlc.toml`.
fn example_dirs() -> [PathBuf; 2] {
    ["quickstart", "conventions"].map(|name| manifest_dir().join("docs/examples").join(name))
}

/// Reads a text file with line endings normalized to `\n`.
fn read_text(path: &Path) -> Result<String> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(text.replace("\r\n", "\n"))
}

fn readme() -> Result<String> {
    read_text(&manifest_dir().join("README.md"))
}

/// One fenced code block of the README.
#[derive(Debug)]
struct Block {
    /// The info string after the opening fence (`rust`, `toml`, ...).
    lang: String,
    /// The `<!-- ... -->` comment on the line before the fence, without the
    /// comment markers.
    marker: Option<String>,
    body: String,
    /// 1-based line of the opening fence.
    line: usize,
}

fn blocks(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut previous: Option<&str> = None;
    let mut open: Option<Block> = None;
    for (index, line) in text.lines().enumerate() {
        match open.take() {
            Some(block) if line.trim_end() == "```" => blocks.push(block),
            Some(mut block) => {
                block.body.push_str(line);
                block.body.push('\n');
                open = Some(block);
            }
            None => {
                if let Some(lang) = line.strip_prefix("```") {
                    let marker = previous
                        .map(str::trim)
                        .and_then(|prev| prev.strip_prefix("<!--"))
                        .and_then(|prev| prev.strip_suffix("-->"))
                        .map(|marker| marker.trim().to_owned());
                    open = Some(Block {
                        lang: lang.trim().to_owned(),
                        marker,
                        body: String::new(),
                        line: index + 1,
                    });
                }
            }
        }
        previous = Some(line);
    }
    blocks
}

fn rok_sqlc(dir: &Path, args: &[&str]) -> Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
        .args(args)
        .current_dir(dir)
        .output()
        .context("failed to run rok-sqlc")
}

/// The names serde lists after `expected` in the error for an unknown key or
/// value in `payload`, as the binary reports it.
fn expected_names(payload: &str) -> Result<BTreeSet<String>> {
    let dir = TempDir::new()?;
    let output = rok_sqlc(dir.path(), &["g", "-j", payload])?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let Some((_, expected)) = stderr.split_once("expected ") else {
        bail!("no `expected` list for {payload}:\n{stderr}");
    };
    let expected = expected.split(" at line").next().unwrap_or(expected);
    Ok(expected
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect())
}

fn keys(table: &toml::Table) -> BTreeSet<String> {
    table.keys().cloned().collect()
}

#[test]
fn every_flag_and_alias_in_the_code_is_documented_and_no_other() -> Result<()> {
    let readme = readme()?;
    let mut cli = rok_sqlc::cli::command();
    cli.build();
    let mut in_code = BTreeSet::new();
    let mut commands = vec![&cli];
    commands.extend(cli.get_subcommands());
    for command in &commands {
        for arg in command.get_arguments().filter(|arg| !arg.is_positional()) {
            let short = arg.get_short().map(|short| format!("-{short}"));
            let long = arg.get_long().map(|long| format!("--{long}"));
            in_code.insert(short.into_iter().chain(long).collect::<Vec<_>>().join(", "));
        }
    }

    // Each flag row's first cell: `` `-s`, `--schema <PATH>` ``.
    let documented: BTreeSet<String> = readme
        .lines()
        .filter(|line| line.starts_with("| `-"))
        .map(|line| {
            let cell = line.split('|').nth(1).unwrap_or_default();
            cell.split('`')
                .skip(1)
                .step_by(2)
                .filter_map(|code| code.split_whitespace().next())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect();
    assert_eq!(documented, in_code, "README flag table vs. clap");

    for subcommand in cli.get_subcommands() {
        let names = std::iter::once(subcommand.get_name()).chain(subcommand.get_all_aliases());
        for name in names {
            ensure!(
                readme.contains(&format!("`{name}`")),
                "README does not mention the command `{name}`"
            );
        }
    }
    Ok(())
}

#[test]
fn every_config_key_in_the_code_is_documented_and_no_other() -> Result<()> {
    let readme = readme()?;
    let reference = blocks(&readme)
        .into_iter()
        .find(|block| block.marker.as_deref() == Some("config-reference"))
        .context("README has no `<!-- config-reference -->` toml block")?;
    let reference: toml::Table = reference.body.parse()?;
    let conventions = reference["conventions"]
        .as_table()
        .context("the reference has a [conventions] table")?;

    assert_eq!(
        keys(&reference),
        expected_names(r#"{"not_a_key": 1}"#)?,
        "top-level keys"
    );
    assert_eq!(
        keys(conventions),
        expected_names(r#"{"conventions": {"not_a_key": 1}}"#)?,
        "[conventions] keys"
    );
    let preset: BTreeSet<String> = [conventions["preset"]
        .as_str()
        .unwrap_or_default()
        .to_owned()]
    .into_iter()
    .collect();
    assert_eq!(
        preset,
        expected_names(r#"{"conventions": {"preset": "not_a_preset"}}"#)?,
        "presets"
    );
    let derive: BTreeSet<String> = reference["derive"]
        .as_array()
        .context("`derive` is a list")?
        .iter()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect();
    assert_eq!(
        derive,
        expected_names(r#"{"derive": ["not_a_derive"]}"#)?,
        "derive names"
    );
    Ok(())
}

/// Every TOML and JSON sample not taken from a file is a configuration the
/// binary accepts.
#[test]
fn every_config_sample_is_accepted() -> Result<()> {
    let readme = readme()?;
    let dir = TempDir::new()?;
    let mut checked = 0;
    for block in blocks(&readme) {
        let sourced = block
            .marker
            .as_deref()
            .is_some_and(|marker| marker.starts_with("source:"));
        if sourced {
            continue;
        }
        let file = dir.path().join("sample");
        fs::write(&file, &block.body)?;
        let (flag, value, rejection) = match block.lang.as_str() {
            "toml" => ("-c", file.display().to_string(), "invalid config file"),
            "json" => ("-j", format!("@{}", file.display()), "invalid JSON payload"),
            _ => continue,
        };
        let output = rok_sqlc(dir.path(), &["g", "-n", flag, &value])?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        ensure!(
            !stderr.contains(rejection),
            "README.md:{}: the {} sample is rejected:\n{stderr}",
            block.line,
            block.lang
        );
        checked += 1;
    }
    ensure!(checked >= 2, "expected TOML and JSON samples in the README");
    Ok(())
}

#[test]
fn every_code_sample_comes_from_its_source() -> Result<()> {
    let readme = readme()?;
    let mut problems = Vec::new();
    for block in blocks(&readme) {
        let source = block
            .marker
            .as_deref()
            .and_then(|marker| marker.strip_prefix("source:"))
            .map(str::trim);
        let Some(source) = source else {
            if block.lang == "rust" {
                problems.push(format!(
                    "README.md:{}: a rust block without `<!-- source: <path> -->`",
                    block.line
                ));
            }
            continue;
        };
        let text = read_text(&manifest_dir().join(source))?;
        if !text.contains(&block.body) {
            problems.push(format!(
                "README.md:{}: the block is not in {source}:\n{}",
                block.line, block.body
            ));
        }
    }
    ensure!(problems.is_empty(), "{}", problems.join("\n\n"));
    Ok(())
}

/// Renders `dir` like the README's trees: entries sorted by name,
/// directories with a trailing `/`.
fn render_tree(dir: &Path, prefix: &str, out: &mut String) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    let count = entries.len();
    for (index, entry) in entries.into_iter().enumerate() {
        let last = index + 1 == count;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry.file_type()?.is_dir();
        out.push_str(prefix);
        out.push_str(if last { "└── " } else { "├── " });
        out.push_str(&name);
        if is_dir {
            out.push('/');
        }
        out.push('\n');
        if is_dir {
            let nested = format!("{prefix}{}", if last { "    " } else { "│   " });
            render_tree(&entry.path(), &nested, out)?;
        }
    }
    Ok(())
}

#[test]
fn every_file_tree_matches_its_directory() -> Result<()> {
    let readme = readme()?;
    let mut checked = 0;
    for block in blocks(&readme) {
        let Some(dir) = block
            .marker
            .as_deref()
            .and_then(|marker| marker.strip_prefix("tree:"))
            .map(str::trim)
        else {
            continue;
        };
        let name = Path::new(dir)
            .file_name()
            .context("a tree names a directory")?
            .to_string_lossy();
        let mut rendered = format!("{name}/\n");
        render_tree(&manifest_dir().join(dir), "", &mut rendered)?;
        assert_eq!(
            block.body, rendered,
            "README.md:{}: tree of {dir}",
            block.line
        );
        checked += 1;
    }
    ensure!(checked > 0, "expected a file tree in the README");
    Ok(())
}

/// The committed example output is exactly what the current binary
/// generates from the example's SQL, run as the README says.
#[test]
fn the_examples_have_no_drift() -> Result<()> {
    for dir in example_dirs() {
        let output = rok_sqlc(&dir, &["g", "--check"])?;
        ensure!(
            output.status.success(),
            "{} drifted; run `rok-sqlc g` there:\n{}{}",
            dir.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// The example crates, generated code and the README's calling code alike,
/// build against SQLx 0.9 and pass clippy.
#[test]
fn the_examples_compile() -> Result<()> {
    for dir in example_dirs() {
        if let Some(reason) = common::generated_code_skip_reason(&dir) {
            eprintln!("{reason}");
            return Ok(());
        }
        let status = Command::new("cargo")
            .args(["clippy", "--quiet", "--all-targets", "--", "-Dwarnings"])
            .current_dir(&dir)
            .env(
                "CARGO_TARGET_DIR",
                manifest_dir().join("target/fixture-target"),
            )
            .status()
            .with_context(|| format!("failed to run cargo clippy in {}", dir.display()))?;
        ensure!(status.success(), "{} does not compile", dir.display());
    }
    Ok(())
}
