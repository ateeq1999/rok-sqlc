use super::super::test_support::{column, defaulted, generated, nullable, table};
use super::*;

#[test]
fn static_create_when_no_column_has_a_default() {
    let rendered = render(&table(
        vec![
            column("id", "uuid::Uuid"),
            column("email", "String"),
            nullable("note", "String"),
        ],
        &["id"],
    ));
    assert!(rendered.contains("use super::models::{NewUser, UpdateUser, User};"));
    assert!(rendered.contains(
        "pub async fn create<'e, E>(executor: E, input: NewUser) -> Result<User, sqlx::Error>"
    ));
    assert!(rendered.contains(
            "        INSERT INTO users (id, email, note)\n        VALUES ($1, $2, $3)\n        RETURNING\n            id,\n            email,\n            note\n"
        ));
    assert!(rendered.contains(
            "    .bind(input.id)\n    .bind(input.email)\n    .bind(input.note)\n    .fetch_one(executor)\n"
        ));
    assert!(!rendered.contains("QueryBuilder::<sqlx::Postgres>::new(\"INSERT"));
}

#[test]
fn dynamic_create_omits_defaulted_columns_from_columns_and_binds_alike() {
    let rendered = render(&table(
        vec![
            generated(column("id", "i64")),
            column("email", "String"),
            defaulted(column("created_at", "chrono::DateTime<chrono::Utc>")),
            defaulted(nullable("tag", "String")),
        ],
        &["id"],
    ));
    assert!(rendered.contains(
        "    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new(\"INSERT INTO users (\");\n"
    ));
    assert!(rendered.contains("    columns.push(\"email\");\n    if input.created_at.is_some() {\n        columns.push(\"created_at\");\n    }\n    if input.tag.is_some() {\n        columns.push(\"tag\");\n    }\n"));
    assert!(rendered.contains("    values.push_bind(input.email);\n    if let Some(value) = input.created_at {\n        values.push_bind(value);\n    }\n    if let Some(value) = input.tag {\n        values.push_bind(value);\n    }\n"));
    assert!(!rendered.contains("columns.push(\"id\")"));
    assert!(
        !rendered.contains("DEFAULT VALUES"),
        "a required column means the list is never empty"
    );
    assert!(rendered.contains("    query.build_query_as::<User>().fetch_one(executor).await\n"));
}

#[test]
fn dynamic_create_falls_back_to_default_values() {
    let rendered = render(&table(
        vec![
            generated(column("id", "i64")),
            defaulted(column("hits", "i32")),
            defaulted(column("created_at", "chrono::DateTime<chrono::Utc>")),
        ],
        &["id"],
    ));
    assert!(rendered.contains("    if input.hits.is_none() && input.created_at.is_none() {\n        return sqlx::query_as::<_, User>(\n            r#\"\n            INSERT INTO users\n            DEFAULT VALUES\n            RETURNING\n"));
    assert!(rendered.contains("        .fetch_one(executor)\n        .await;\n    }\n"));
}

#[test]
fn update_uses_query_builder_binds_and_guards_empty_patch() {
    let rendered = render(&table(
        vec![
            column("order_id", "uuid::Uuid"),
            column("product_id", "uuid::Uuid"),
            column("quantity", "i32"),
            nullable("note", "String"),
        ],
        &["order_id", "product_id"],
    ));
    assert!(rendered.contains(
            "pub async fn update<'e, E>(\n    executor: E,\n    key: UserKey,\n    input: UpdateUser,\n) -> Result<Option<User>, sqlx::Error>"
        ));
    assert!(rendered.contains("    if input.is_empty() {\n        return super::read::get_by_id(executor, key).await;\n    }\n"));
    assert!(rendered.contains("    if let Some(value) = input.quantity {\n        assignments.push(\"quantity = \");\n        assignments.push_bind_unseparated(value);\n    }\n"));
    assert!(rendered.contains("    query.push(\" WHERE order_id = \");\n    query.push_bind(key.order_id);\n    query.push(\" AND product_id = \");\n    query.push_bind(key.product_id);\n"));
    assert!(
        !rendered.contains("sets.len()"),
        "placeholders come from QueryBuilder, not arithmetic"
    );
    assert!(rendered.contains("fetch_optional(executor)"));
}

#[test]
fn update_assignments_follow_column_order() {
    let rendered = render(&table(
        vec![
            column("id", "i64"),
            column("alpha", "String"),
            column("beta", "String"),
            column("gamma", "String"),
        ],
        &["id"],
    ));
    let positions: Vec<usize> = ["alpha", "beta", "gamma"]
        .iter()
        .map(|name| {
            rendered
                .find(&format!("assignments.push(\"{name} = \")"))
                .unwrap()
        })
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn quotes_reserved_identifiers_in_dynamic_sql() {
    let rendered = render(&table(
        vec![
            generated(column("select", "i64")),
            defaulted(column("from", "String")),
            column("user", "String"),
        ],
        &["select"],
    ));
    assert!(rendered.contains("columns.push(r#\"\"from\"\"#);"));
    assert!(rendered.contains("assignments.push(r#\"\"user\" = \"#);"));
    assert!(rendered.contains("query.push(r#\" WHERE \"select\" = \"#);"));
}

#[test]
fn delete_reports_whether_a_row_was_removed() {
    let rendered = render(&table(
        vec![column("id", "i64"), column("email", "String")],
        &["id"],
    ));
    assert!(
        rendered.contains(
            "pub async fn delete<'e, E>(executor: E, id: i64) -> Result<bool, sqlx::Error>"
        )
    );
    assert!(rendered.contains(
            "    let result = sqlx::query(\n        r#\"\n        DELETE FROM users\n        WHERE id = $1\n        \"#,\n    )\n    .bind(id)\n    .execute(executor)\n    .await?;\n    Ok(result.rows_affected() > 0)\n"
        ));
}

#[test]
fn without_primary_key_only_create() {
    let rendered = render(&table(vec![column("email", "String")], &[]));
    assert!(rendered.contains("use super::models::{NewUser, User};"));
    assert!(rendered.contains("pub async fn create"));
    assert!(!rendered.contains("pub async fn update"));
    assert!(!rendered.contains("pub async fn delete"));
    assert!(rendered.contains("No primary key detected"));
}

#[test]
fn without_insertable_columns_skips_create() {
    let rendered = render(&table(vec![generated(column("id", "i64"))], &["id"]));
    assert!(rendered.contains("use super::models::User;"));
    assert!(!rendered.contains("pub async fn create"));
    assert!(rendered.contains("No insertable columns detected"));
    assert!(rendered.contains("No updatable columns detected"));
    assert!(rendered.contains("pub async fn delete"));
}

#[test]
fn long_model_imports_wrap_like_rustfmt() {
    let names = |base: &str| {
        vec![
            base.to_owned(),
            format!("New{base}"),
            format!("Update{base}"),
        ]
    };
    assert_eq!(
        use_models(&names("User")[..1]),
        "use super::models::User;\n"
    );
    assert_eq!(
        use_models(&names("User")),
        "use super::models::{User, NewUser, UpdateUser};\n"
    );
    assert_eq!(
        use_models(&names("ControlledSubstanceEntry")),
        "use super::models::{\n    ControlledSubstanceEntry, NewControlledSubstanceEntry, UpdateControlledSubstanceEntry,\n};\n"
    );
    // 99 columns on one line: rustfmt already wraps it (98 is the most).
    let at_limit = [
        "ExpenseCategory",
        "ExpenseCategoryId",
        "NewExpenseCategory",
        "UpdateExpenseCategory",
    ]
    .map(str::to_owned);
    assert_eq!(
        use_models(&at_limit),
        "use super::models::{\n    ExpenseCategory, ExpenseCategoryId, NewExpenseCategory, UpdateExpenseCategory,\n};\n"
    );
    // A list too long for one indented line is packed greedily, as
    // rustfmt's mixed import layout does.
    let packed = [
        "CustomFieldDefinition",
        "CustomFieldDefinitionId",
        "NewCustomFieldDefinition",
        "UpdateCustomFieldDefinition",
    ]
    .map(str::to_owned);
    assert_eq!(
        use_models(&packed),
        "use super::models::{\n    CustomFieldDefinition, CustomFieldDefinitionId, NewCustomFieldDefinition,\n    UpdateCustomFieldDefinition,\n};\n"
    );
    let huge = "VeryLongTableNameThatKeepsGoingAndGoingAndGoing";
    assert!(use_models(&names(huge)).contains(&format!("{{\n    {huge},\n    New{huge},\n")));
}

#[test]
fn output_is_deterministic() {
    let build = || {
        render(&table(
            vec![column("id", "i64"), column("email", "String")],
            &["id"],
        ))
    };
    assert_eq!(build(), build());
}
