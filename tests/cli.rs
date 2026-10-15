use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::Result;
use tempfile::TempDir;

const SIMPLE_SCHEMA: &str = "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);";
const MIXED_SCHEMA: &str = "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);\n\
     CREATE TABLE spans (id BIGSERIAL PRIMARY KEY, duration INTERVAL NOT NULL);";

fn run(args: &[&str]) -> Result<()> {
    let mut full: Vec<String> = vec!["rok-sqlc".to_owned()];
    full.extend(args.iter().map(|arg| arg.to_string()));
    rok_sqlc::cli::run_from(full)
}

fn output_dir(dir: &TempDir) -> PathBuf {
    dir.path().join("out")
}

#[test]
fn generates_resource_modules() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ])?;

    let out = output_dir(&dir);
    assert!(out.join("users/mod.rs").exists());
    assert!(out.join("users/models.rs").exists());
    assert!(out.join("users/read.rs").exists());
    assert!(out.join("users/write.rs").exists());
    assert!(out.join("mod.rs").exists());
    assert!(out.join("schema.rs").exists());

    let models = fs::read_to_string(out.join("users/models.rs"))?;
    assert!(models.contains("pub struct User {"));
    assert!(models.contains("pub id: uuid::Uuid,"));
    assert!(models.contains("pub email: String,"));

    let read = fs::read_to_string(out.join("users/read.rs"))?;
    assert!(read.contains(
        "        SELECT\n            id,\n            email\n        FROM users\n        WHERE id = $1\n"
    ));

    let write = fs::read_to_string(out.join("users/write.rs"))?;
    assert!(write.contains("pub async fn create<'e, E>(executor: E, input: NewUser)"));
    assert!(write.contains(
        "pub async fn update<'e, E>(\n    executor: E,\n    id: uuid::Uuid,\n    input: UpdateUser,\n)"
    ));
    assert!(write.contains("pub async fn delete<'e, E>(executor: E, id: uuid::Uuid)"));

    for generated in [&read, &write] {
        assert!(
            !generated.contains("PgPool"),
            "generated code must use the generic executor, not a concrete pool"
        );
    }

    let db_mod = fs::read_to_string(out.join("mod.rs"))?;
    assert!(db_mod.contains("pub mod users;"));
    assert!(db_mod.contains("\"uuid\""));
    Ok(())
}

#[test]
fn generates_composite_keys_arrays_and_time() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(
        &schema,
        "CREATE TABLE line_items (
            order_id UUID NOT NULL,
            product_id UUID NOT NULL,
            labels TEXT[] NOT NULL,
            weekdays INTEGER[],
            cut_off TIME NOT NULL,
            PRIMARY KEY (order_id, product_id)
        );",
    )?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ])?;

    let out = output_dir(&dir);
    let models = fs::read_to_string(out.join("line_items/models.rs"))?;
    assert!(models.contains("pub labels: Vec<String>,"));
    assert!(models.contains("pub weekdays: Option<Vec<i32>>,"));
    assert!(models.contains("pub cut_off: chrono::NaiveTime,"));

    let read = fs::read_to_string(out.join("line_items/read.rs"))?;
    assert!(read.contains(
        "pub async fn get_by_id<'e, E>(\n    executor: E,\n    order_id: uuid::Uuid,\n    product_id: uuid::Uuid,\n)"
    ));
    assert!(read.contains("WHERE order_id = $1 AND product_id = $2"));
    assert!(read.contains(".bind(order_id)"));
    assert!(read.contains(".bind(product_id)"));
    Ok(())
}

#[test]
fn generates_write_helpers_with_defaults_and_serial() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(
        &schema,
        "CREATE TABLE events (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            note TEXT,
            is_active BOOLEAN NOT NULL DEFAULT TRUE,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );",
    )?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ])?;

    let out = output_dir(&dir);
    let models = fs::read_to_string(out.join("events/models.rs"))?;
    assert!(models.contains("pub struct NewEvent {"));
    assert!(!models.contains("pub id: Option<i64>"));
    assert!(models.contains("pub name: String,"));
    assert!(models.contains("pub note: Option<String>,"));
    assert!(models.contains("pub is_active: Option<bool>,"));
    assert!(models.contains("pub struct UpdateEvent {"));
    assert!(models.contains("pub note: Option<Option<String>>,"));

    let write = fs::read_to_string(out.join("events/write.rs"))?;
    assert!(!write.contains("columns.push(\"id\")"));
    assert!(write.contains("    columns.push(\"name\");\n"));
    assert!(
        write
            .contains("    if input.is_active.is_some() {\n        columns.push(\"is_active\");\n")
    );
    assert!(write.contains(
        "    if let Some(value) = input.is_active {\n        values.push_bind(value);\n"
    ));
    // `name` is required, so the column list is never empty.
    assert!(!write.contains("DEFAULT VALUES"));
    Ok(())
}

#[test]
fn generates_products_fixture_with_singular_names_and_schema_qualified_sql() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    let inventory = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("modules/inventory/schema.sql"),
    )?;
    fs::write(
        &schema,
        inventory.replace("CREATE TABLE products", "CREATE TABLE catalog.products"),
    )?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
        "--resource",
        "products",
    ])?;

    let out = output_dir(&dir);
    let models = fs::read_to_string(out.join("products/models.rs"))?;
    assert!(models.contains(
        "/// A row of the `catalog.products` table.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct Product {
    pub id: uuid::Uuid,
    pub sku: String,
    pub name: String,
    pub category_id: i64,
    pub supplier_id: Option<uuid::Uuid>,
    pub price_cents: i64,
    pub stock_on_hand: i32,
    pub reorder_level: i16,
    pub weight_kg: Option<f64>,
    pub discontinued: bool,
    pub released_on: Option<chrono::NaiveDate>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
"
    ));
    assert!(models.contains("pub struct NewProduct {"));
    assert!(models.contains("pub struct UpdateProduct {"));

    let read = fs::read_to_string(out.join("products/read.rs"))?;
    assert!(read.contains(
        "pub async fn get_by_id<'e, E>(executor: E, id: uuid::Uuid) -> Result<Option<Product>, sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query_as::<_, Product>(
        r#\"
        SELECT
            id,
            sku,
            name,
            category_id,
            supplier_id,
            price_cents,
            stock_on_hand,
            reorder_level,
            weight_kg,
            discontinued,
            released_on,
            created_at
        FROM catalog.products
        WHERE id = $1
        \"#,
    )
    .bind(id)
    .fetch_optional(executor)
    .await
}
"
    ));

    let write = fs::read_to_string(out.join("products/write.rs"))?;
    assert!(write.contains("new(\"UPDATE catalog.products SET \")"));
    assert!(write.contains("DELETE FROM catalog.products"));
    Ok(())
}

#[test]
fn generates_decimal_columns_with_dependency_hints() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(
        &schema,
        "CREATE TABLE money (id BIGSERIAL PRIMARY KEY, amount NUMERIC(18, 2) NOT NULL, fee DECIMAL);",
    )?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ])?;

    let out = output_dir(&dir);
    let models = fs::read_to_string(out.join("money/models.rs"))?;
    assert!(models.contains("pub amount: rust_decimal::Decimal,"));
    assert!(models.contains("pub fee: Option<rust_decimal::Decimal>,"));

    let db_mod = fs::read_to_string(out.join("mod.rs"))?;
    assert!(db_mod.contains("\"rust_decimal\""));
    assert!(db_mod.contains("rust_decimal = \"1\""));
    Ok(())
}

#[test]
fn dry_run_writes_nothing() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
        "--dry-run",
    ])?;

    assert!(!output_dir(&dir).exists());
    Ok(())
}

#[test]
fn strict_failure_writes_nothing() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, MIXED_SCHEMA)?;

    let result = run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ]);

    assert!(result.is_err());
    assert!(!output_dir(&dir).exists());
    Ok(())
}

#[test]
fn lenient_skips_unsupported_and_generates_rest() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, MIXED_SCHEMA)?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
        "--lenient",
    ])?;

    let out = output_dir(&dir);
    assert!(out.join("users/models.rs").exists());
    assert!(!out.join("spans").exists());
    Ok(())
}

#[test]
fn resource_filter_limits_generation() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(
        &schema,
        format!("{SIMPLE_SCHEMA}\nCREATE TABLE posts (id BIGSERIAL PRIMARY KEY);"),
    )?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
        "--resource",
        "posts",
    ])?;

    let out = output_dir(&dir);
    assert!(out.join("posts/models.rs").exists());
    assert!(!out.join("users").exists());
    Ok(())
}

#[test]
fn unknown_resource_fails() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;

    let result = run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
        "--resource",
        "nope",
    ]);

    assert!(result.is_err());
    Ok(())
}

#[test]
fn overwrite_protection_blocks_foreign_files() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;

    let out = output_dir(&dir);
    let models = out.join("users/models.rs");
    fs::create_dir_all(models.parent().unwrap())?;
    fs::write(&models, "// hand-written, precious\n")?;

    let guarded = run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ]);
    assert!(guarded.is_err());
    assert_eq!(fs::read_to_string(&models)?, "// hand-written, precious\n");
    assert!(!out.join("users/read.rs").exists());

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
        "--force",
    ])?;
    assert!(fs::read_to_string(&models)?.contains("// @generated by rok-sqlc"));
    Ok(())
}

#[test]
fn regeneration_is_idempotent() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;

    let out = output_dir(&dir).to_str().unwrap().to_owned();
    let args = [
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        out.as_str(),
    ];
    run(&args)?;
    let models = fs::read_to_string(output_dir(&dir).join("users/models.rs"))?;
    run(&args)?;
    assert_eq!(
        fs::read_to_string(output_dir(&dir).join("users/models.rs"))?,
        models
    );
    Ok(())
}

#[test]
fn invalid_sql_fails() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, "CREATE TABLE broken (")?;

    let result = run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ]);

    assert!(result.is_err());
    Ok(())
}

#[test]
fn snapshot_escapes_raw_string_delimiters() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(
        &schema,
        format!("{SIMPLE_SCHEMA}\n-- note: \"# inside comment\n"),
    )?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ])?;

    let snapshot = fs::read_to_string(output_dir(&dir).join("schema.rs"))?;
    assert!(snapshot.contains("r##\""));
    Ok(())
}

#[test]
fn filtered_runs_merge_root_mod() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(
        &schema,
        format!("{SIMPLE_SCHEMA}\nCREATE TABLE posts (id BIGSERIAL PRIMARY KEY);"),
    )?;

    let out = output_dir(&dir).to_str().unwrap().to_owned();
    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        &out,
        "--resource",
        "users",
    ])?;
    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        &out,
        "--resource",
        "posts",
    ])?;

    let db_mod = fs::read_to_string(output_dir(&dir).join("mod.rs"))?;
    assert!(db_mod.contains("pub mod users;"));
    assert!(db_mod.contains("pub mod posts;"));
    assert!(output_dir(&dir).join("users/models.rs").exists());
    assert!(output_dir(&dir).join("posts/models.rs").exists());
    Ok(())
}

#[test]
fn binary_dry_run_lists_planned_files() -> Result<()> {
    use std::process::Command;

    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;

    let output = Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
        .args([
            "generate",
            "--schema",
            schema.to_str().unwrap(),
            "--output",
            dir.path().join("out").to_str().unwrap(),
            "--dry-run",
        ])
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("would write"));
    assert!(!dir.path().join("out").exists());
    Ok(())
}

#[test]
fn missing_schema_file_fails() {
    let result = run(&["generate", "--schema", "does-not-exist.sql"]);
    assert!(result.is_err());
}

const QUERY_FILE: &str = "-- name: GetUserByEmail :maybe_one
SELECT id, email FROM users WHERE email = $1;

-- name: DeleteUser :execrows
DELETE FROM users WHERE id = $1;
";

#[test]
fn generates_query_functions_with_queries_flag() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    let queries = dir.path().join("queries.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;
    fs::write(&queries, QUERY_FILE)?;
    let out = output_dir(&dir);

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--queries",
        queries.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ])?;

    let generated = fs::read_to_string(out.join("queries.rs"))?;
    assert!(generated.starts_with("// @generated by rok-sqlc"));
    assert!(generated.contains("pub async fn get_user_by_email<'e, E>("));
    assert!(generated.contains("-> Result<Option<super::users::models::User>, sqlx::Error>"));
    assert!(generated.contains(
        "pub async fn delete_user<'e, E>(executor: E, id: uuid::Uuid) -> Result<u64, sqlx::Error>"
    ));
    assert!(fs::read_to_string(out.join("mod.rs"))?.contains("pub mod queries;"));

    // A later run without --queries keeps declaring the generated module.
    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ])?;
    assert!(fs::read_to_string(out.join("mod.rs"))?.contains("pub mod queries;"));
    Ok(())
}

#[test]
fn unsupported_query_fails_strictly_and_is_skipped_leniently() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    let queries = dir.path().join("queries.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;
    fs::write(
        &queries,
        format!("{QUERY_FILE}\n-- name: Broken :many\nSELECT colour FROM users;\n"),
    )?;
    let out = output_dir(&dir);
    let args = |lenient: bool| {
        let mut args = vec![
            "generate".to_owned(),
            "--schema".to_owned(),
            schema.to_str().unwrap().to_owned(),
            "--queries".to_owned(),
            queries.to_str().unwrap().to_owned(),
            "--output".to_owned(),
            out.to_str().unwrap().to_owned(),
        ];
        if lenient {
            args.push("--lenient".to_owned());
        }
        args
    };

    let strict_args = args(false);
    let error = run(&strict_args.iter().map(String::as_str).collect::<Vec<_>>()).unwrap_err();
    assert!(error.to_string().contains("1 unsupported query"));
    assert!(!out.exists(), "a strict failure writes nothing");

    let lenient_args = args(true);
    run(&lenient_args.iter().map(String::as_str).collect::<Vec<_>>())?;
    let generated = fs::read_to_string(out.join("queries.rs"))?;
    assert!(generated.contains("pub async fn get_user_by_email"));
    assert!(!generated.contains("broken"));
    Ok(())
}

fn run_binary(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
        .args(args)
        .output()
        .expect("failed to spawn rok-sqlc binary")
}

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

fn read_tree(root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, String>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                walk(root, &path, files)?;
            } else {
                let rel = path.strip_prefix(root)?.to_path_buf();
                files.insert(rel, fs::read_to_string(&path)?);
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files)?;
    Ok(files)
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

// --- rok-sqlc.toml -----------------------------------------------------------

fn run_binary_in(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rok-sqlc"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("failed to spawn rok-sqlc binary")
}

#[test]
fn config_file_in_a_parent_directory_is_discovered_and_resolved_against_its_directory() -> Result<()>
{
    let dir = TempDir::new()?;
    fs::create_dir_all(dir.path().join("db"))?;
    fs::write(dir.path().join("db/schema.sql"), SIMPLE_SCHEMA)?;
    fs::write(
        dir.path().join("rok-sqlc.toml"),
        "schema = [\"db/schema.sql\"]\noutput = \"gen\"\n",
    )?;
    let sub = dir.path().join("crates/app");
    fs::create_dir_all(&sub)?;

    let output = run_binary_in(&sub, &["g"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dir.path().join("gen/users/models.rs").exists());
    assert!(!sub.join("gen").exists());
    Ok(())
}

#[test]
fn explicit_config_file_is_used_instead_of_discovery() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), SIMPLE_SCHEMA)?;
    // The discoverable file points at a schema that doesn't exist.
    fs::write(
        dir.path().join("rok-sqlc.toml"),
        "schema = \"missing.sql\"\noutput = \"wrong\"\n",
    )?;
    fs::create_dir_all(dir.path().join("config"))?;
    fs::write(
        dir.path().join("config/other.toml"),
        "schema = \"../schema.sql\"\noutput = \"../right\"\n",
    )?;

    let output = run_binary_in(dir.path(), &["g", "-c", "config/other.toml"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dir.path().join("right/users/models.rs").exists());
    assert!(!dir.path().join("wrong").exists());
    Ok(())
}

#[test]
fn flags_override_the_config_file() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), MIXED_SCHEMA)?;
    let config = dir.path().join("rok-sqlc.toml");
    fs::write(
        &config,
        "schema = \"schema.sql\"\noutput = \"from-file\"\nresource = \"spans\"\n",
    )?;
    let flag_out = output_dir(&dir);

    run(&[
        "g",
        "--config",
        config.to_str().unwrap(),
        "--output",
        flag_out.to_str().unwrap(),
        "--resource",
        "users",
    ])?;

    assert!(flag_out.join("users/models.rs").exists());
    assert!(!flag_out.join("spans").exists());
    assert!(!dir.path().join("from-file").exists());
    Ok(())
}

#[test]
fn unknown_config_key_names_the_key_and_the_file() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(dir.path().join("schema.sql"), SIMPLE_SCHEMA)?;
    let config = dir.path().join("rok-sqlc.toml");
    // `output` stays valid so that, if the typo were ignored, the run would
    // write into the temp dir rather than the working directory.
    fs::write(
        &config,
        "schema = \"schema.sql\"\noutput = \"gen\"\nresorce = \"users\"\n",
    )?;

    let err = run(&["g", "-c", config.to_str().unwrap()]).unwrap_err();

    let message = format!("{err:#}");
    assert!(message.contains("resorce"), "{message}");
    assert!(message.contains(&config.display().to_string()), "{message}");
    assert!(!dir.path().join("gen").exists());
    Ok(())
}

#[test]
fn missing_schema_without_a_config_file_is_a_usage_error() -> Result<()> {
    let dir = TempDir::new()?;
    let output = run_binary_in(dir.path(), &["g"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.starts_with("error: "), "{stderr}");
    assert!(stderr.contains("rok-sqlc.toml"), "{stderr}");
    Ok(())
}
