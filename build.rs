//! Embeds the project templates of `rok-sqlc init --template` (the
//! directories of `templates/`) in the binary, so `init` works from
//! `cargo install`. It writes `$OUT_DIR/templates.rs`: one `Embedded` (see
//! `src/init/template.rs`) per template, its directory and its files, each
//! a `File`: its path (relative, with `/`) and its bytes through
//! `include_bytes!`. A template's
//! top-level `target/` (a build of it in place) is left out; everything
//! else is embedded, and `init` decides what it copies.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn main() -> io::Result<()> {
    let root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets it")).join("templates");
    println!("cargo:rerun-if-changed=templates");
    let mut output = String::from("&[\n");
    for template in sorted_entries(&root)? {
        if !template.is_dir() {
            continue;
        }
        let name = file_name(&template);
        output.push_str(&format!("    Embedded {{ dir: {name:?}, files: &[\n"));
        let mut files = Vec::new();
        collect(&template, &template, &mut files)?;
        files.sort();
        for (relative, absolute) in files {
            output.push_str(&format!(
                "        File {{ path: {relative:?}, bytes: include_bytes!({absolute:?}) }},\n"
            ));
        }
        output.push_str("    ] },\n");
    }
    output.push_str("]\n");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets it"));
    fs::write(out.join("templates.rs"), output)
}

/// `dir`'s entries, sorted; none when `dir` doesn't exist.
fn sorted_entries(dir: &Path) -> io::Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries = fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<_>>>()?;
    entries.sort();
    Ok(entries)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Every file under `dir` as (its path relative to `template`, with `/`,
/// and its absolute path), but those of `template`'s `target/`.
fn collect(template: &Path, dir: &Path, files: &mut Vec<(String, String)>) -> io::Result<()> {
    for path in sorted_entries(dir)? {
        if path.is_dir() {
            if dir == template && file_name(&path) == "target" {
                continue;
            }
            collect(template, &path, files)?;
        } else {
            let relative = path
                .strip_prefix(template)
                .map_err(io::Error::other)?
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            files.push((relative, path.to_string_lossy().into_owned()));
        }
    }
    Ok(())
}
