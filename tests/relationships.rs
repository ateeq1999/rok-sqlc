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

#[test]
fn only_to_one_relationships_of_a_keyed_table_get_a_joined_read() -> Result<()> {
    let (dir, result) = generate(
        "CREATE TABLE posts (id BIGINT PRIMARY KEY, title TEXT NOT NULL);
CREATE TABLE comments (id BIGINT PRIMARY KEY, post_id BIGINT NOT NULL REFERENCES posts (id));
CREATE TABLE post_views (post_id BIGINT NOT NULL REFERENCES posts (id));
",
        "",
    );
    result?;
    let comments = loaders(dir.path(), "comments").expect("comments has relationships");
    assert!(
        comments.contains(
            "pub async fn get_with_post<'e, E>(\n    executor: E,\n    id: CommentId,\n)"
        ),
        "{comments}"
    );
    assert!(
        comments.contains("pub struct CommentWithPost {"),
        "{comments}"
    );
    let posts = loaders(dir.path(), "posts").expect("posts has relationships");
    assert!(posts.contains("pub async fn load_comments<"), "{posts}");
    assert!(
        !posts.contains("get_with_"),
        "a to-many is not joined:\n{posts}"
    );
    let views = loaders(dir.path(), "post_views").expect("post_views has relationships");
    assert!(views.contains("pub async fn load_post<"), "{views}");
    assert!(
        !views.contains("get_with_"),
        "no primary key to read a row by:\n{views}"
    );
    Ok(())
}
