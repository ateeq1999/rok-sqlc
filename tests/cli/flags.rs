//! Command-line flags, short aliases, help, and usage errors.

use super::*;

#[test]
fn help_and_version_print_to_stdout_and_exit_zero() {
    for args in [
        &["--help"][..],
        &["-h"],
        &["--version"],
        &["-V"],
        &["g", "--help"],
        &["generate", "-h"],
    ] {
        let output = run_binary(args);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "`rok-sqlc {}` exited with {:?}; stderr: {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !stdout.trim().is_empty(),
            "`rok-sqlc {}` printed nothing to stdout",
            args.join(" ")
        );
    }
    let version = run_binary(&["-V"]);
    assert!(String::from_utf8_lossy(&version.stdout).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn short_aliases_generate_the_same_files_as_long_flags() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    let queries = dir.path().join("queries.sql");
    fs::write(&schema, MIXED_SCHEMA)?;
    fs::write(&queries, QUERY_FILE)?;
    let schema = schema.to_str().unwrap();
    let queries = queries.to_str().unwrap();
    let long_out = dir.path().join("long");
    let short_out = dir.path().join("short");

    run(&[
        "generate",
        "--schema",
        schema,
        "--output",
        long_out.to_str().unwrap(),
        "--resource",
        "users",
        "--queries",
        queries,
        "--lenient",
    ])?;
    run(&[
        "g",
        "-s",
        schema,
        "-o",
        short_out.to_str().unwrap(),
        "-r",
        "users",
        "-q",
        queries,
        "-l",
    ])?;

    let long_tree = read_tree(&long_out)?;
    assert!(long_tree.contains_key(&PathBuf::from("users").join("models.rs")));
    assert!(long_tree.contains_key(&PathBuf::from("queries.rs")));
    assert_eq!(long_tree, read_tree(&short_out)?);
    Ok(())
}

#[test]
fn short_dry_run_matches_long_dry_run_through_the_binary() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;
    let schema = schema.to_str().unwrap();
    let out = output_dir(&dir);
    let out = out.to_str().unwrap();

    let long = run_binary(&["generate", "--schema", schema, "--output", out, "--dry-run"]);
    let short = run_binary(&["g", "-s", schema, "-o", out, "-n"]);

    assert!(long.status.success());
    assert_eq!(long.status.code(), short.status.code());
    assert_eq!(long.stdout, short.stdout);
    assert!(String::from_utf8_lossy(&short.stdout).contains("would write"));
    assert!(!output_dir(&dir).exists(), "dry run must not write files");
    Ok(())
}

#[test]
fn short_force_overwrites_hand_written_files() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;
    let out = output_dir(&dir);
    fs::create_dir_all(&out)?;
    fs::write(out.join("mod.rs"), "// hand written\n")?;
    let schema = schema.to_str().unwrap();

    assert!(run(&["g", "-s", schema, "-o", out.to_str().unwrap()]).is_err());
    run(&["g", "-s", schema, "-o", out.to_str().unwrap(), "-f"])?;

    assert!(fs::read_to_string(out.join("mod.rs"))?.contains("pub mod users;"));
    Ok(())
}

/// Short flags reserved for flags that later tickets introduce, with the long
/// flag each one belongs to. Nothing else may take these letters.
const RESERVED: [(char, &str); 3] = [('j', "json"), ('c', "config"), ('k', "check")];

/// The command line and every subcommand under it, each paired with its
/// display path (`rok-sqlc`, `rok-sqlc generate`, ...). Clap's built-in
/// `help` subcommand is left out.
fn all_commands() -> Vec<(String, clap::Command)> {
    fn collect(cmd: &clap::Command, path: String, out: &mut Vec<(String, clap::Command)>) {
        out.push((path.clone(), cmd.clone()));
        for sub in subcommands(cmd) {
            collect(sub, format!("{path} {}", sub.get_name()), out);
        }
    }
    let mut cmd = rok_sqlc::cli::command();
    cmd.build();
    let mut out = Vec::new();
    collect(&cmd, "rok-sqlc".to_owned(), &mut out);
    out
}

fn subcommands(cmd: &clap::Command) -> impl Iterator<Item = &clap::Command> {
    cmd.get_subcommands().filter(|sub| sub.get_name() != "help")
}

/// Every named flag of `cmd` with its short letter, if it has one.
fn flags(cmd: &clap::Command) -> Vec<(String, Option<char>)> {
    cmd.get_arguments()
        .filter(|arg| !arg.is_positional())
        .map(|arg| (arg.get_id().to_string(), arg.get_short()))
        .collect()
}

#[test]
fn every_flag_has_a_short_alias() {
    for (path, cmd) in all_commands() {
        for (id, short) in flags(&cmd) {
            assert!(short.is_some(), "`{path} --{id}` has no short alias");
        }
    }
}

#[test]
fn no_two_flags_of_a_command_share_a_short_alias() {
    for (path, cmd) in all_commands() {
        let mut owners: BTreeMap<char, String> = BTreeMap::new();
        for (id, short) in flags(&cmd) {
            let Some(short) = short else { continue };
            if let Some(other) = owners.insert(short, id.clone()) {
                panic!("`{path}`: `-{short}` is used by both `--{other}` and `--{id}`");
            }
        }
    }
}

#[test]
fn reserved_short_aliases_are_used_only_by_their_flags() {
    for (path, cmd) in all_commands() {
        for (id, short) in flags(&cmd) {
            let reserved = RESERVED.iter().find(|(letter, _)| Some(*letter) == short);
            if let Some((letter, owner)) = reserved {
                assert_eq!(
                    id, *owner,
                    "`{path} --{id}` takes `-{letter}`, which is reserved for `--{owner}`"
                );
            }
        }
    }
}

#[test]
fn every_subcommand_has_a_single_letter_alias() {
    for (path, cmd) in all_commands() {
        for sub in subcommands(&cmd) {
            assert!(
                sub.get_all_aliases()
                    .any(|alias| alias.chars().count() == 1),
                "`{path} {}` has no single-letter alias",
                sub.get_name()
            );
        }
    }
}

#[test]
fn no_two_subcommands_share_a_name_or_alias() {
    for (path, cmd) in all_commands() {
        let mut owners: BTreeMap<&str, &str> = BTreeMap::new();
        for sub in subcommands(&cmd) {
            let name = sub.get_name();
            for token in std::iter::once(name).chain(sub.get_all_aliases()) {
                if let Some(other) = owners.insert(token, name) {
                    panic!("`{path}`: `{token}` names both `{other}` and `{name}`");
                }
            }
        }
    }
}

#[test]
fn usage_errors_print_one_error_prefix_and_exit_non_zero() {
    for args in [&["generate"][..], &["g", "--no-such-flag"]] {
        let output = run_binary(args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success(),
            "`rok-sqlc {}` should fail",
            args.join(" ")
        );
        assert!(
            stderr.starts_with("error: "),
            "`rok-sqlc {}` stderr: {stderr}",
            args.join(" ")
        );
        assert_eq!(
            stderr.matches("error:").count(),
            1,
            "`rok-sqlc {}` doubled the error prefix: {stderr}",
            args.join(" ")
        );
    }
}
