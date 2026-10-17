//! The tenant column and the session settings.

use super::*;

/// Every `pub async fn` signature in `source`, as `name(params)` on one line.
fn signatures(source: &str) -> Vec<String> {
    source
        .split("pub async fn ")
        .skip(1)
        .map(|rest| {
            let name = rest.split(['<', '(']).next().unwrap();
            let params = &rest[rest.find('(').unwrap() + 1..rest.find(") ->").unwrap()];
            let params: Vec<&str> = params
                .split(",\n")
                .flat_map(|chunk| chunk.split(", "))
                .map(str::trim)
                .filter(|param| !param.is_empty())
                .collect();
            format!("{name}({})", params.join(", "))
        })
        .collect()
}

const TENANT: &str = "[conventions]\ntenant_column = \"organization_id\"\n";

#[test]
fn tenant_tables_take_the_tenant_right_after_the_executor() -> Result<()> {
    let (dir, config) = project(TENANT)?;

    generate(&config)?;

    let read = signatures(&resource_file(&dir, "notes", "read.rs")?);
    let write = signatures(&resource_file(&dir, "notes", "write.rs")?);
    assert_eq!(
        read,
        [
            "get_by_id(executor: E, organization_id: uuid::Uuid, id: NoteId)",
            "exists_by_id(executor: E, organization_id: uuid::Uuid, id: NoteId)",
            "get_by_code(executor: E, organization_id: uuid::Uuid, code: &str)",
            "list(executor: E, organization_id: uuid::Uuid)",
            "list_page(executor: E, organization_id: uuid::Uuid, limit: i64, offset: i64)",
            "count(executor: E, organization_id: uuid::Uuid)",
            "search(executor: E, organization_id: uuid::Uuid, filter: NoteFilter, sort: Sort<NoteSortKey>, page: Page)",
            "search_after(executor: E, organization_id: uuid::Uuid, filter: NoteFilter, after: Option<NoteId>, limit: Limit)",
        ]
    );
    assert_eq!(
        write,
        [
            "create(executor: E, organization_id: uuid::Uuid, input: NewNote)",
            "update(executor: E, organization_id: uuid::Uuid, id: NoteId, input: UpdateNote)",
            "delete(executor: E, organization_id: uuid::Uuid, id: NoteId)",
        ]
    );

    let models = models(&dir, "notes")?;
    assert_eq!(
        struct_fields(&models, "Note"),
        ["id", "organization_id", "code", "body"]
    );
    assert_eq!(struct_fields(&models, "NewNote"), ["id", "code", "body"]);
    assert_eq!(struct_fields(&models, "UpdateNote"), ["code", "body"]);
    Ok(())
}

#[test]
fn a_primary_key_including_the_tenant_takes_the_tenant_once() -> Result<()> {
    let (dir, config) = project(TENANT)?;

    generate(&config)?;

    let read = signatures(&resource_file(&dir, "members", "read.rs")?);
    assert_eq!(
        read[0],
        "get_by_id(executor: E, organization_id: uuid::Uuid, id: uuid::Uuid)"
    );
    assert!(
        read.iter()
            .all(|signature| signature.matches("organization_id").count() == 1),
        "{read:?}"
    );
    let write = signatures(&resource_file(&dir, "members", "write.rs")?);
    assert_eq!(
        write,
        [
            "create(executor: E, organization_id: uuid::Uuid, input: NewMember)",
            "update(executor: E, organization_id: uuid::Uuid, id: uuid::Uuid, input: UpdateMember)",
            "delete(executor: E, organization_id: uuid::Uuid, id: uuid::Uuid)",
        ]
    );
    let models = models(&dir, "members")?;
    assert_eq!(struct_fields(&models, "NewMember"), ["id", "email"]);
    assert!(!models.contains("MemberKey"), "{models}");
    Ok(())
}

#[test]
fn without_the_tenant_convention_signatures_are_unchanged() -> Result<()> {
    let (plain, plain_config) = project("")?;
    let (tenant, tenant_config) = project(TENANT)?;

    generate(&plain_config)?;
    generate(&tenant_config)?;

    assert_eq!(
        signatures(&resource_file(&plain, "notes", "read.rs")?)[0],
        "get_by_id(executor: E, id: NoteId)"
    );
    assert_eq!(
        signatures(&resource_file(&plain, "members", "read.rs")?)[0],
        "get_by_id(executor: E, key: MemberKey)"
    );
    // A table without the tenant column is generated as if no convention
    // were configured.
    assert_eq!(
        tree(&plain.path().join("out").join("tags"))?,
        tree(&tenant.path().join("out").join("tags"))?
    );
    assert!(!plain.path().join("out/session.rs").exists());
    assert!(!tenant.path().join("out/session.rs").exists());
    Ok(())
}

#[test]
fn session_settings_generate_transaction_local_helpers() -> Result<()> {
    let (dir, config) = project(
        "[conventions]\ntenant_setting = \"app.organization_id\"\nuser_setting = \"app.user_id\"\n",
    )?;

    generate(&config)?;

    let session = fs::read_to_string(dir.path().join("out/session.rs"))?;
    assert_eq!(
        signatures(&session),
        [
            "set_tenant(connection: &mut sqlx::PgConnection, id: impl std::fmt::Display)",
            "set_user(connection: &mut sqlx::PgConnection, id: impl std::fmt::Display)",
        ]
    );
    assert!(session.contains("SELECT set_config('app.organization_id', $1, true)"));
    assert!(session.contains("SELECT set_config('app.user_id', $1, true)"));
    let db_mod = fs::read_to_string(dir.path().join("out/mod.rs"))?;
    assert!(db_mod.contains("pub mod session;\n"), "{db_mod}");

    let (tenant_only, config) =
        project("[conventions]\ntenant_setting = \"app.organization_id\"\n")?;
    generate(&config)?;
    let session = fs::read_to_string(tenant_only.path().join("out/session.rs"))?;
    assert_eq!(
        signatures(&session),
        ["set_tenant(connection: &mut sqlx::PgConnection, id: impl std::fmt::Display)"]
    );
    Ok(())
}

/// The variant names of `pub enum {name}` in `source`, in order.
fn enum_variants(source: &str, name: &str) -> Vec<String> {
    let start = source
        .find(&format!("pub enum {name} {{"))
        .unwrap_or_else(|| panic!("no enum {name} in:\n{source}"));
    let body = &source[start..];
    let body = &body[body.find('{').unwrap() + 1..body.find("\n}").unwrap()];
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("///"))
        .map(|line| line.trim_end_matches(',').to_owned())
        .collect()
}

#[test]
fn the_tenant_column_is_neither_a_filter_field_nor_a_sort_key() -> Result<()> {
    let (dir, config) = project(TENANT)?;

    generate(&config)?;

    let notes = resource_file(&dir, "notes", "filter.rs")?;
    assert_eq!(struct_fields(&notes, "NoteFilter"), ["id", "code", "body"]);
    assert_eq!(enum_variants(&notes, "NoteSortKey"), ["Id", "Code", "Body"]);
    assert!(!notes.contains("organization_id"), "{notes}");
    let members = resource_file(&dir, "members", "filter.rs")?;
    assert_eq!(struct_fields(&members, "MemberFilter"), ["id", "email"]);
    assert_eq!(enum_variants(&members, "MemberSortKey"), ["Id", "Email"]);
    Ok(())
}

#[test]
fn a_nullable_tenant_column_gets_no_null_check_field() -> Result<()> {
    let (dir, config) = project_with(
        "CREATE TABLE notes (id UUID PRIMARY KEY, organization_id UUID, body TEXT);",
        TENANT,
    )?;

    generate(&config)?;

    let notes = resource_file(&dir, "notes", "filter.rs")?;
    assert_eq!(
        struct_fields(&notes, "NoteFilter"),
        ["id", "body", "body_is_null"]
    );
    assert_eq!(enum_variants(&notes, "NoteSortKey"), ["Id", "Body"]);
    assert!(!notes.contains("organization_id"), "{notes}");
    Ok(())
}

#[test]
fn without_the_tenant_convention_the_column_is_filtered_and_sorted() -> Result<()> {
    let (dir, config) = project("")?;

    generate(&config)?;

    let notes = resource_file(&dir, "notes", "filter.rs")?;
    assert_eq!(
        struct_fields(&notes, "NoteFilter"),
        ["id", "organization_id", "code", "body"]
    );
    assert_eq!(
        enum_variants(&notes, "NoteSortKey"),
        ["Id", "OrganizationId", "Code", "Body"]
    );
    Ok(())
}
