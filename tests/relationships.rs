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
fn a_relationship_named_like_a_filter_field_fails_generation() -> Result<()> {
    // `category_id` makes a belongs-to named `category`, and the search
    // filter already has a `category` field for the text column.
    let (dir, result) = generate(
        "CREATE TABLE categories (id BIGINT PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE products (
    id BIGINT PRIMARY KEY,
    category TEXT,
    category_id BIGINT NOT NULL REFERENCES categories (id)
);
",
        "",
    );

    let message = format!("{:#}", result.unwrap_err());
    assert!(
        message.contains(
            "the `category` relationship of `products` collides with the search filter's `category` field for column `category`"
        ),
        "{message}"
    );
    assert!(!dir.path().join("out").exists());
    Ok(())
}

#[test]
fn relationship_filters_skip_tenant_tables_from_untenanted_ones() -> Result<()> {
    let (dir, result) = generate(
        "CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT NOT NULL);
CREATE TABLE notes (
    id BIGINT PRIMARY KEY,
    tenant_id UUID NOT NULL,
    author_id BIGINT NOT NULL REFERENCES users (id)
);
",
        "[conventions]\ntenant_column = \"tenant_id\"\n",
    );
    result?;
    let notes = fs::read_to_string(dir.path().join("out/notes/filter.rs"))?;
    assert!(
        notes.contains("pub author: Option<Box<UserFilter>>,"),
        "{notes}"
    );
    // A `users` search has no tenant to scope `notes` by, so it gets no
    // `notes` filter.
    let users = fs::read_to_string(dir.path().join("out/users/filter.rs"))?;
    assert!(!users.contains("pub notes:"), "{users}");
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

fn writes(dir: &Path, module: &str) -> String {
    fs::read_to_string(dir.join("out").join(module).join("write.rs")).unwrap()
}

const ACCESS_SCHEMA: &str = "CREATE TABLE organizations (id UUID PRIMARY KEY);
CREATE TABLE users (id BIGINT PRIMARY KEY);
CREATE TABLE roles (id BIGINT PRIMARY KEY, organization_id UUID NOT NULL REFERENCES organizations (id));
CREATE TABLE permissions (key TEXT PRIMARY KEY);
CREATE TABLE role_permissions (
    id BIGSERIAL PRIMARY KEY,
    organization_id UUID NOT NULL REFERENCES organizations (id),
    role_id BIGINT NOT NULL REFERENCES roles (id),
    permission_key TEXT NOT NULL REFERENCES permissions (key),
    limits TEXT,
    UNIQUE (organization_id, role_id, permission_key)
);
CREATE TABLE user_roles (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users (id),
    role_id BIGINT NOT NULL REFERENCES roles (id)
);
";

#[test]
fn a_join_table_relates_its_two_tables_many_to_many() -> Result<()> {
    let (dir, result) = generate(
        ACCESS_SCHEMA,
        "[conventions]\ntenant_column = \"organization_id\"\n",
    );
    result?;
    let roles = loaders(dir.path(), "roles").expect("roles has relationships");
    assert!(roles.contains("pub async fn load_permissions<"), "{roles}");
    assert!(
        roles.contains("pub async fn load_role_permissions<"),
        "{roles}"
    );
    let permissions = loaders(dir.path(), "permissions").expect("the other side");
    assert!(
        permissions.contains("pub async fn load_roles<"),
        "{permissions}"
    );

    let join = writes(dir.path(), "role_permissions");
    for function in ["attach", "detach", "sync"] {
        assert!(
            join.contains(&format!("pub async fn {function}<")),
            "{function}: {join}"
        );
    }
    Ok(())
}

#[test]
fn two_foreign_keys_without_a_unique_pair_are_not_a_join_table() -> Result<()> {
    let (dir, result) = generate(ACCESS_SCHEMA, "");
    result?;
    let users = loaders(dir.path(), "users").expect("users has user_roles");
    assert!(!users.contains("pub async fn load_roles<"), "{users}");
    let roles = loaders(dir.path(), "roles").expect("roles has relationships");
    assert!(!roles.contains("pub async fn load_users<"), "{roles}");
    let user_roles = writes(dir.path(), "user_roles");
    assert!(!user_roles.contains("pub async fn attach<"), "{user_roles}");
    Ok(())
}

#[test]
fn opting_out_one_side_of_a_many_to_many_removes_both() -> Result<()> {
    let tenant = "[conventions]\ntenant_column = \"organization_id\"\n";
    let (dir, result) = generate(
        ACCESS_SCHEMA,
        &format!("{tenant}[relationships.roles]\npermissions = false\n"),
    );
    result?;
    let roles = loaders(dir.path(), "roles").expect("roles keeps its has-many");
    assert!(!roles.contains("load_permissions"), "{roles}");
    assert!(
        roles.contains("pub async fn load_role_permissions<"),
        "{roles}"
    );
    let permissions = loaders(dir.path(), "permissions").expect("its has-many stays");
    assert!(
        !permissions.contains("load_roles"),
        "no `roles` either: {permissions}"
    );
    // The join table's own foreign keys stay, and with them its writes.
    let join = writes(dir.path(), "role_permissions");
    assert!(join.contains("pub async fn sync<"), "{join}");

    let (renamed, result) = generate(
        ACCESS_SCHEMA,
        &format!("{tenant}[relationships.permissions]\nroles = \"granted_roles\"\n"),
    );
    result?;
    let permissions = loaders(renamed.path(), "permissions").expect("renamed");
    assert!(
        permissions.contains("pub async fn load_granted_roles<"),
        "{permissions}"
    );
    Ok(())
}

#[test]
fn opting_out_a_join_tables_foreign_key_ends_the_many_to_many() -> Result<()> {
    let (dir, result) = generate(
        ACCESS_SCHEMA,
        "[conventions]\ntenant_column = \"organization_id\"\n[relationships.role_permissions]\nrole = false\n",
    );
    result?;
    let roles = loaders(dir.path(), "roles").expect("roles keeps user_roles");
    assert!(!roles.contains("load_permissions"), "{roles}");
    assert!(!roles.contains("load_role_permissions"), "{roles}");
    let permissions = loaders(dir.path(), "permissions").expect("its has-many stays");
    assert!(!permissions.contains("load_roles"), "{permissions}");
    let join = writes(dir.path(), "role_permissions");
    assert!(!join.contains("pub async fn attach<"), "{join}");
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

#[test]
fn joined_reads_follow_renamed_and_opted_out_relationships() -> Result<()> {
    let (dir, result) = generate(
        INVOICES_SCHEMA,
        "[relationships.invoices]\ncreated_by_user = \"author\"\napproved_by_user = false\n",
    );
    result?;
    let invoices = loaders(dir.path(), "invoices").expect("invoices has relationships");
    assert!(
        invoices.contains("pub async fn get_with_author<"),
        "{invoices}"
    );
    assert!(
        invoices.contains("pub struct InvoiceWithAuthor {"),
        "{invoices}"
    );
    assert!(!invoices.contains("get_with_created_by_user"), "{invoices}");
    assert!(
        !invoices.contains("get_with_approved_by_user"),
        "{invoices}"
    );
    assert!(!invoices.contains("WithApprovedByUser"), "{invoices}");
    Ok(())
}

#[test]
fn relationship_filters_follow_renames_and_opt_outs() -> Result<()> {
    let (dir, result) = generate(
        INVOICES_SCHEMA,
        "[relationships.invoices]\ncreated_by_user = \"author\"\napproved_by_user = false\n[relationships.users]\ninvoices_by_created_by_user = \"authored_invoices\"\n",
    );
    result?;
    let invoices = fs::read_to_string(dir.path().join("out/invoices/filter.rs"))?;
    assert!(
        invoices.contains("pub author: Option<Box<UserFilter>>,"),
        "{invoices}"
    );
    assert!(!invoices.contains("pub created_by_user:"), "{invoices}");
    assert!(!invoices.contains("pub approved_by_user:"), "{invoices}");
    let users = fs::read_to_string(dir.path().join("out/users/filter.rs"))?;
    assert!(
        users.contains("pub authored_invoices: Option<Box<InvoiceFilter>>,"),
        "{users}"
    );
    assert!(!users.contains("pub invoices_by_"), "{users}");
    Ok(())
}

#[test]
fn renaming_a_relationship_resolves_a_filter_field_collision() -> Result<()> {
    let schema = "CREATE TABLE categories (id BIGINT PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE products (
    id BIGINT PRIMARY KEY,
    category TEXT,
    category_id BIGINT NOT NULL REFERENCES categories (id)
);
";
    let (_dir, result) = generate(schema, "");
    let message = format!("{:#}", result.unwrap_err());
    assert!(
        message.contains("rename the relationship in `relationships.products`"),
        "{message}"
    );

    let (dir, result) = generate(
        schema,
        "[relationships.products]\ncategory = \"category_row\"\n",
    );
    result?;
    let products = fs::read_to_string(dir.path().join("out/products/filter.rs"))?;
    assert!(
        products.contains("pub category_row: Option<Box<CategoryFilter>>,"),
        "{products}"
    );
    Ok(())
}
