//! Relationship inference (ADR 0014), through the CLI: which foreign keys
//! become relationships and when names collide. The loaders' behavior
//! against PostgreSQL is proven in `tests/fixture-app`.

use std::fs;
use std::path::Path;

use anyhow::Result;
use tempfile::TempDir;

fn run(args: &[&str]) -> Result<()> {
    let mut full: Vec<String> = vec!["rok-sqlc".to_owned()];
    full.extend(args.iter().map(|arg| arg.to_string()));
    rok_sqlc::cli::run_from(full)
}

/// Generates `schema` into `out/` of a temp dir, with `extra_toml` in its
/// `rok-sqlc.toml`.
fn generate(schema: &str, extra_toml: &str) -> (TempDir, Result<()>) {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("schema.sql"), schema).unwrap();
    let config = dir.path().join("rok-sqlc.toml");
    fs::write(
        &config,
        format!("schema = \"schema.sql\"\noutput = \"out\"\n{extra_toml}"),
    )
    .unwrap();
    let result = run(&["g", "-c", config.to_str().unwrap()]);
    (dir, result)
}

fn loaders(dir: &Path, module: &str) -> Option<String> {
    fs::read_to_string(dir.join("out").join(module).join("relationships.rs")).ok()
}

const TENANT_SCHEMA: &str = "CREATE TABLE organizations (id UUID PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE notes (
    id UUID PRIMARY KEY,
    organization_id UUID NOT NULL REFERENCES organizations (id),
    body TEXT NOT NULL
);
";

#[test]
fn the_tenant_columns_foreign_key_is_not_a_relationship() -> Result<()> {
    let (plain, result) = generate(TENANT_SCHEMA, "");
    result?;
    let notes = loaders(plain.path(), "notes").expect("notes has relationships");
    assert!(notes.contains("pub async fn load_organization<"), "{notes}");
    let organizations = loaders(plain.path(), "organizations").expect("the inverse");
    assert!(
        organizations.contains("pub async fn load_notes<"),
        "{organizations}"
    );

    let (scoped, result) = generate(
        TENANT_SCHEMA,
        "[conventions]\ntenant_column = \"organization_id\"\n",
    );
    result?;
    assert_eq!(loaders(scoped.path(), "notes"), None);
    assert_eq!(loaders(scoped.path(), "organizations"), None);
    let mod_rs = fs::read_to_string(scoped.path().join("out/notes/mod.rs"))?;
    assert!(!mod_rs.contains("relationships"), "{mod_rs}");
    Ok(())
}

#[test]
fn a_foreign_key_to_a_table_outside_the_run_is_not_a_relationship() -> Result<()> {
    let (dir, result) = generate(
        "CREATE TABLE notes (id UUID PRIMARY KEY, owner_id UUID REFERENCES legacy.users (id));",
        "",
    );
    result?;
    assert_eq!(loaders(dir.path(), "notes"), None);
    Ok(())
}

#[test]
fn colliding_relationship_names_fail_generation() -> Result<()> {
    // `users.profile_id` is a belongs-to named `profile`, and the unique
    // `profiles.user_id` makes a has-one on `users` named `profile` too.
    let (dir, result) = generate(
        "CREATE TABLE users (id BIGINT PRIMARY KEY, profile_id BIGINT REFERENCES profiles (id));
CREATE TABLE profiles (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL UNIQUE REFERENCES users (id)
);
",
        "",
    );

    let message = format!("{:#}", result.unwrap_err());
    assert!(
        message.contains("relationships of `users` collide: `profile`"),
        "{message}"
    );
    assert!(
        message.contains("belongs-to `profiles` through `users.profile_id`"),
        "{message}"
    );
    assert!(
        message.contains("has-one `profiles` through `profiles.user_id`"),
        "{message}"
    );
    assert!(!dir.path().join("out").exists());
    Ok(())
}

/// `invoices` references `users` twice, so both sides have two relationships.
const INVOICES_SCHEMA: &str = "CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT NOT NULL);
CREATE TABLE invoices (
    id BIGINT PRIMARY KEY,
    created_by_user_id BIGINT NOT NULL REFERENCES users (id),
    approved_by_user_id BIGINT REFERENCES users (id)
);
";

#[test]
fn a_renamed_relationship_generates_under_its_new_name() -> Result<()> {
    let (dir, result) = generate(
        INVOICES_SCHEMA,
        "[relationships.invoices]\ncreated_by_user = \"author\"\n[relationships.users]\ninvoices_by_created_by_user = \"authored_invoices\"\n",
    );
    result?;
    let invoices = loaders(dir.path(), "invoices").expect("invoices has relationships");
    assert!(invoices.contains("pub async fn load_author<"), "{invoices}");
    assert!(!invoices.contains("load_created_by_user"), "{invoices}");
    assert!(
        invoices.contains("pub async fn load_approved_by_user<"),
        "{invoices}"
    );
    let users = loaders(dir.path(), "users").expect("users has relationships");
    assert!(
        users.contains("pub async fn load_authored_invoices<"),
        "{users}"
    );
    assert!(
        !users.contains("load_invoices_by_created_by_user"),
        "{users}"
    );
    Ok(())
}

#[test]
fn an_opted_out_relationship_generates_nothing_on_either_side() -> Result<()> {
    // Opted out from the belongs-to side: its inverse on `users` goes too.
    let (dir, result) = generate(
        INVOICES_SCHEMA,
        "[relationships.invoices]\napproved_by_user = false\n",
    );
    result?;
    let invoices = loaders(dir.path(), "invoices").expect("invoices keeps one");
    assert!(!invoices.contains("load_approved_by_user"), "{invoices}");
    assert!(
        invoices.contains("pub async fn load_created_by_user<"),
        "{invoices}"
    );
    let users = loaders(dir.path(), "users").expect("users keeps one");
    assert!(
        !users.contains("load_invoices_by_approved_by_user"),
        "{users}"
    );
    assert!(
        users.contains("pub async fn load_invoices_by_created_by_user<"),
        "{users}"
    );

    // Opted out from the inverse side, through the JSON payload: the
    // belongs-to goes too, and a table left without relationships gets no
    // `relationships.rs` and no `relationships` module.
    let config = dir.path().join("rok-sqlc.toml");
    run(&[
        "g",
        "-c",
        config.to_str().unwrap(),
        "-j",
        r#"{"relationships":{"users":{"invoices_by_created_by_user":false}}}"#,
    ])?;
    assert_eq!(loaders(dir.path(), "invoices"), None);
    assert_eq!(loaders(dir.path(), "users"), None);
    let mod_rs = fs::read_to_string(dir.path().join("out/invoices/mod.rs"))?;
    assert!(!mod_rs.contains("relationships"), "{mod_rs}");
    Ok(())
}

fn failure(schema: &str, extra_toml: &str) -> String {
    let (dir, result) = generate(schema, extra_toml);
    let message = format!("{:#}", result.expect_err("generation fails"));
    assert!(!dir.path().join("out").exists(), "{message}");
    message
}

#[test]
fn unknown_relationships_fail_naming_the_config_entry() {
    let message = failure(
        INVOICES_SCHEMA,
        "[relationships.invoices]\ncreated_by = \"author\"\n",
    );
    assert!(
        message.contains("`relationships.invoices.created_by` in the configuration names no relationship of `invoices`"),
        "{message}"
    );
    assert!(
        message.contains("`created_by_user`, `approved_by_user`"),
        "{message}"
    );

    let message = failure(
        INVOICES_SCHEMA,
        "[relationships.invoice]\ncreated_by_user = false\n",
    );
    assert!(
        message
            .contains("`relationships.invoice` in the configuration names no table of the schema"),
        "{message}"
    );

    let message = failure(
        INVOICES_SCHEMA,
        "[relationships.invoices]\ncreated_by_user = true\n",
    );
    assert!(message.contains("`true` changes nothing"), "{message}");
    assert!(message.contains("rok-sqlc.toml"), "{message}");
}

#[test]
fn colliding_or_invalid_renames_fail_naming_the_config_entry() {
    let message = failure(
        INVOICES_SCHEMA,
        "[relationships.invoices]\ncreated_by_user = \"approved_by_user\"\n",
    );
    assert!(
        message.contains("relationships of `invoices` collide: `approved_by_user`"),
        "{message}"
    );
    assert!(
        message.contains("`relationships.invoices.created_by_user` in the configuration"),
        "{message}"
    );

    let message = failure(
        INVOICES_SCHEMA,
        "[relationships.invoices]\ncreated_by_user = \"Author\"\n",
    );
    assert!(
        message.contains("`relationships.invoices.created_by_user` in the configuration renames the relationship to `Author`, which is not a snake_case Rust identifier"),
        "{message}"
    );

    let message = failure(
        INVOICES_SCHEMA,
        "[relationships.invoices]\ncreated_by_user = false\n[relationships.users]\ninvoices_by_created_by_user = \"authored\"\n",
    );
    assert!(
        message.contains("`relationships.users.invoices_by_created_by_user` in the configuration renames a relationship that `relationships.invoices.created_by_user` opts out"),
        "{message}"
    );
}

#[test]
fn configuration_resolves_an_inferred_collision() -> Result<()> {
    let schema =
        "CREATE TABLE users (id BIGINT PRIMARY KEY, profile_id BIGINT REFERENCES profiles (id));
CREATE TABLE profiles (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL UNIQUE REFERENCES users (id)
);
";
    let message = failure(schema, "");
    assert!(
        message.contains("rename one of them, or opt one out, in `relationships.users`"),
        "{message}"
    );

    let (dir, result) = generate(schema, "[relationships.users]\nprofile = \"own_profile\"\n");
    result?;
    let users = loaders(dir.path(), "users").expect("users has relationships");
    assert!(users.contains("pub async fn load_own_profile<"), "{users}");
    assert!(users.contains("pub async fn load_profile<"), "{users}");
    Ok(())
}

#[test]
fn a_resource_run_accepts_entries_for_tables_it_does_not_generate() -> Result<()> {
    let (dir, result) = generate(
        INVOICES_SCHEMA,
        "[relationships.invoices]\ncreated_by_user = \"author\"\n",
    );
    result?;
    fs::remove_dir_all(dir.path().join("out"))?;
    let config = dir.path().join("rok-sqlc.toml");
    run(&["g", "-c", config.to_str().unwrap(), "-r", "users"])?;
    assert_eq!(loaders(dir.path(), "users"), None);

    let config_text = fs::read_to_string(&config)?.replace("created_by_user =", "created_by =");
    fs::write(&config, config_text)?;
    let message = format!(
        "{:#}",
        run(&["g", "-c", config.to_str().unwrap(), "-r", "users"]).unwrap_err()
    );
    assert!(
        message.contains("`relationships.invoices.created_by`"),
        "{message}"
    );
    Ok(())
}
