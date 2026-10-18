//! The `[api]` configuration (ADR 0016): read from the config file, the JSON
//! payload and `-a/--api-output`, layered like the other options, and
//! checked against the schema. It adds `db/pool.rs` to the data layer and
//! the shared tools beside the features directory.

use super::*;

const API_SCHEMA: &str = "CREATE TABLE users (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    age INT,
    active BOOLEAN NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);
CREATE TABLE posts (
    id BIGSERIAL PRIMARY KEY,
    author_id UUID NOT NULL REFERENCES users (id),
    editor_id UUID REFERENCES users (id),
    title TEXT NOT NULL
);
CREATE TABLE audit_log (id BIGSERIAL PRIMARY KEY, note TEXT);
";

const API_OPTIONS: &str = "derive = [\"serde\", \"validate\"]\nextractors = [\"actix_web\"]\n";

/// Every `[api]` key, valid against [`API_SCHEMA`].
const FULL_API: &str = "[api]
output = \"src/features\"
db_path = \"crate::db\"
prefix = \"/api/v2\"
search_endpoint = true
exclude = [\"audit_log\"]

[api.skip]
posts = [\"remove\", \"restore\"]

[api.routes]
users = \"/members\"

[api.access]
users = \"admin\"
posts = \"public\"

[api.owner]
posts = \"author_id\"

[api.hidden]
users = [\"password_hash\"]

[api.search]
users = [\"email\"]

[api.sort]
users = [\"email\", \"age\", \"created_at\"]
";

/// A project dir holding the schema and a `rok-sqlc.toml` with `extra`.
fn api_project(extra: &str) -> Result<TempDir> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), API_SCHEMA)?;
    fs::write(
        dir.path().join("rok-sqlc.toml"),
        format!("schema = \"schema.sql\"\noutput = \"out\"\n{API_OPTIONS}\n{extra}"),
    )?;
    Ok(dir)
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn an_api_section_adds_only_the_pool_and_the_shared_tools() -> Result<()> {
    let with_api = api_project(FULL_API)?;
    let without = api_project("")?;

    assert_success(&run_binary_in(with_api.path(), &["g"]));
    assert_success(&run_binary_in(without.path(), &["g"]));

    // The data layer gains `pool.rs`, declared in `mod.rs`, and nothing else.
    let mut with_pool = read_tree(&with_api.path().join("out"))?;
    assert!(with_pool.remove(Path::new("pool.rs")).is_some());
    let mut plain = read_tree(&without.path().join("out"))?;
    for files in [&mut with_pool, &mut plain] {
        if let Some(text) = files.get_mut(Path::new("mod.rs")) {
            *text = text
                .replace("pub mod pool;\n", "")
                .replace("//   tracing = \"0.1\"\n", "");
        }
    }
    assert_eq!(with_pool, plain);
    let shared = read_tree(&with_api.path().join("src/shared"))?;
    assert_eq!(shared.len(), 5, "{:?}", shared.keys());
    assert!(!without.path().join("src").exists());
    assert!(!with_api.path().join("src/features").exists());
    Ok(())
}

/// `-a/--api-output` turns the API on; it needs `serde`, `validate` and the
/// `actix_web` extractor, wherever they come from.
#[test]
fn the_api_output_flag_needs_serde_validate_and_actix_web() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), API_SCHEMA)?;
    let base = ["g", "-s", "schema.sql", "-o", "out"];
    let cases: [&[&str]; 3] = [
        &["-a", "src/features"],
        &["-a", "src/features", "-d", "serde", "-d", "validate"],
        &[
            "--api-output",
            "src/features",
            "-d",
            "serde",
            "-d",
            "validate",
            "-e",
            "axum",
        ],
    ];
    for extra in cases {
        let args: Vec<&str> = base.iter().chain(extra).copied().collect();
        let output = run_binary_in(dir.path(), &args);
        assert!(!output.status.success(), "{args:?} succeeded");
        let stderr = stderr(&output);
        assert!(
            stderr.contains(
                "`api.output` needs `derive = [\"serde\", \"validate\"]` and `actix_web` in `extractors`"
            ),
            "{args:?}: {stderr}"
        );
    }

    let args: Vec<&str> = base
        .iter()
        .chain(&[
            "-a",
            "src/features",
            "-d",
            "serde",
            "-d",
            "validate",
            "-e",
            "actix_web",
        ])
        .copied()
        .collect();
    assert_success(&run_binary_in(dir.path(), &args));
    Ok(())
}

/// Each `[api]` entry is checked against the schema, and a bad one fails the
/// run naming the key, the table and the column.
#[test]
fn api_entries_are_checked_against_the_schema() -> Result<()> {
    let cases = [
        (
            "exclude = [\"comments\"]",
            "`api.exclude` in the configuration names `comments`, which is no generated table of the schema",
        ),
        (
            "[api.skip]\ncomments = [\"list\"]",
            "`api.skip.comments` in the configuration names no generated table of the schema",
        ),
        (
            "[api.access]\ncomments = \"admin\"",
            "`api.access.comments` in the configuration names no generated table of the schema",
        ),
        (
            "[api.owner]\nposts = \"owner_id\"",
            "`api.owner.posts` in the configuration names `owner_id`, which is not a column of `posts`",
        ),
        (
            "[api.owner]\nposts = \"editor_id\"",
            "`api.owner.posts` in the configuration names `editor_id`, which must be `UUID NOT NULL` to be an owner column",
        ),
        (
            "[api.owner]\nposts = \"title\"",
            "`api.owner.posts` in the configuration names `title`, which must be `UUID NOT NULL` to be an owner column",
        ),
        (
            "[api.hidden]\nusers = [\"id\"]",
            "`api.hidden.users` in the configuration names `id`, a key column of `users`, which can't be hidden",
        ),
        (
            "[api.hidden]\nusers = [\"secret\"]",
            "`api.hidden.users` in the configuration names `secret`, which is not a column of `users`",
        ),
        (
            "[api.search]\nusers = [\"age\"]",
            "`api.search.users` in the configuration names `age`, which is not a text column, so `q` can't match it",
        ),
        (
            "[api.sort]\nusers = [\"active\"]",
            "`api.sort.users` in the configuration names `active`, which can't be sorted: sort columns are numbers, dates, times or text",
        ),
        (
            "[api.sort]\n\"public.comments\" = [\"id\"]",
            "`api.sort.\"public.comments\"` in the configuration names no generated table of the schema",
        ),
        (
            "[api.routes]\nusers = \"members\"",
            "`api.routes.users` in the configuration is `members`, but a route base must start with `/`",
        ),
        (
            "[api.routes]\nusers = \"/team\"\nposts = \"/team\"",
            "`api.routes.posts` in the configuration is `/team`, the same route base as `api.routes.users`; route bases must be unique",
        ),
        (
            "[api.routes]\nusers = \"/audit-log\"",
            "`api.routes.users` in the configuration is `/audit-log`, the same route base as the default route base of `audit_log`; route bases must be unique",
        ),
    ];
    for (entry, expected) in cases {
        let dir = api_project(&format!("[api]\noutput = \"src/features\"\n{entry}\n"))?;

        let output = run_binary_in(dir.path(), &["g"]);

        let stderr = stderr(&output);
        assert!(!output.status.success(), "{entry} succeeded");
        assert!(stderr.contains(expected), "{entry}:\n{stderr}");
        assert!(!dir.path().join("out").exists(), "{entry} wrote files");
    }
    Ok(())
}

/// An excluded table has no route base, so another table may take its
/// default one; a run without `api.output` ignores every other `[api]` key.
#[test]
fn excluded_tables_free_their_route_base_and_no_output_ignores_the_rest() -> Result<()> {
    let dir = api_project(
        "[api]\noutput = \"src/features\"\nexclude = [\"audit_log\"]\n\n[api.routes]\nusers = \"/audit-log\"\n",
    )?;
    assert_success(&run_binary_in(dir.path(), &["g"]));

    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), API_SCHEMA)?;
    fs::write(
        dir.path().join("rok-sqlc.toml"),
        "schema = \"schema.sql\"\noutput = \"out\"\n\n[api.owner]\nposts = \"nope\"\n",
    )?;
    assert_success(&run_binary_in(dir.path(), &["g"]));
    Ok(())
}

/// Unknown endpoint and access names fail with the valid ones.
#[test]
fn unknown_skip_and_access_names_list_the_valid_ones() -> Result<()> {
    let cases = [
        (
            "[api.skip]\nusers = [\"delete\"]",
            "`list`, `get_one`, `create`, `update`, `remove`, `search`, `restore`, `lookups`, `relationships`",
        ),
        (
            "[api.access]\nusers = \"owner\"",
            "`public`, `user`, `admin`",
        ),
        ("not_a_key = 1", "`output`, `db_path`, `prefix`"),
    ];
    for (entry, expected) in cases {
        let dir = api_project(&format!("[api]\noutput = \"src/features\"\n{entry}\n"))?;

        let output = run_binary_in(dir.path(), &["g"]);

        let stderr = stderr(&output);
        assert!(!output.status.success(), "{entry} succeeded");
        assert!(stderr.contains(expected), "{entry}:\n{stderr}");
    }
    Ok(())
}

/// The JSON payload takes `[api]` in the same shape and layers over the
/// file: single keys and lists replace, table-keyed entries merge by table.
/// `-a/--api-output` sets `api.output` over both.
#[test]
fn api_keys_layer_across_the_file_the_payload_and_the_flag() -> Result<()> {
    let dir = api_project(
        "[api]\nexclude = [\"audit_log\"]\n\n[api.routes]\nusers = \"/audit-log\"\n\n[api.owner]\nposts = \"editor_id\"\n",
    )?;
    let run = |args: &[&str]| run_binary_in(dir.path(), args);
    let owner_error = "`api.owner.posts` in the configuration names `editor_id`";

    // Without `output`, the file's entries are ignored...
    assert_success(&run(&["g"]));
    // ...until the flag or the payload sets one.
    let failed = run(&["g", "-a", "src/features"]);
    assert!(stderr(&failed).contains(owner_error), "{}", stderr(&failed));
    let failed = run(&["g", "-j", r#"{"api": {"output": "src/features"}}"#]);
    assert!(stderr(&failed).contains(owner_error), "{}", stderr(&failed));

    // An entry for another table merges, keeping the file's bad entry.
    let failed = run(&[
        "g",
        "-a",
        "src/features",
        "-j",
        r#"{"api": {"owner": {"users": "id"}}}"#,
    ]);
    assert!(stderr(&failed).contains(owner_error), "{}", stderr(&failed));

    // An entry for the same table replaces the file's.
    assert_success(&run(&[
        "g",
        "-a",
        "src/features",
        "-j",
        r#"{"api": {"owner": {"posts": "author_id"}}}"#,
    ]));

    // A list replaces the file's whole list: without the exclusion,
    // `audit_log` keeps its default route base, which `users` now takes.
    let failed = run(&[
        "g",
        "-a",
        "src/features",
        "-j",
        r#"{"api": {"exclude": [], "owner": {"posts": "author_id"}}}"#,
    ]);
    assert!(
        stderr(&failed).contains("the same route base as the default route base of `audit_log`"),
        "{}",
        stderr(&failed)
    );
    Ok(())
}
