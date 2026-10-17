use super::super::test_support::{column, table};
use super::*;

/// Renders `table` as if generated on its own.
fn render_alone(table: &Table) -> String {
    render(table, &[])
}

#[test]
fn renders_get_by_id_with_explicit_columns() {
    let rendered = render_alone(&table(
        vec![column("id", "i64"), column("email", "String")],
        &["id"],
    ));
    assert!(rendered.contains(
            "pub async fn get_by_id<'e, E>(executor: E, id: i64) -> Result<Option<User>, sqlx::Error>\nwhere\n    E: sqlx::PgExecutor<'e>,\n{\n"
        ));
    assert!(rendered.contains(
            "    sqlx::query_as::<_, User>(\n        r#\"\n        SELECT\n            id,\n            email\n        FROM users\n        WHERE id = $1\n        \"#,\n    )\n    .bind(id)\n    .fetch_optional(executor)\n    .await\n}\n"
        ));
}

#[test]
fn renders_exists_list_page_and_count() {
    let rendered = render_alone(&table(
        vec![column("id", "i64"), column("email", "String")],
        &["id"],
    ));
    assert!(rendered.contains(
        "pub async fn exists_by_id<'e, E>(executor: E, id: i64) -> Result<bool, sqlx::Error>"
    ));
    assert!(rendered.contains("sqlx::query_scalar::<_, bool>("));
    assert!(rendered.contains("        SELECT EXISTS (\n            SELECT 1\n            FROM users\n            WHERE id = $1\n        )\n"));
    assert!(rendered.contains(
        "        FROM users\n        ORDER BY id\n        \"#,\n    )\n    .fetch_all(executor)"
    ));
    assert!(rendered.contains("        ORDER BY id\n        LIMIT $1\n        OFFSET $2\n"));
    assert!(rendered.contains("    .bind(limit)\n    .bind(offset)\n"));
    assert!(rendered.contains("        SELECT COUNT(*) FROM users\n"));
}

#[test]
fn composite_key_parameters_follow_key_order() {
    let rendered = render_alone(&table(
        vec![
            column("order_id", "uuid::Uuid"),
            column("product_id", "uuid::Uuid"),
            column("quantity", "i32"),
        ],
        &["order_id", "product_id"],
    ));
    assert!(rendered.contains(
            "pub async fn get_by_id<'e, E>(executor: E, key: UserKey) -> Result<Option<User>, sqlx::Error>"
        ));
    assert!(rendered.contains("WHERE order_id = $1 AND product_id = $2"));
    assert!(rendered.contains("    .bind(key.order_id)\n    .bind(key.product_id)\n"));
    assert!(rendered.contains("ORDER BY order_id, product_id"));
}

#[test]
fn quotes_reserved_and_mixed_case_identifiers_and_keeps_schema() {
    let mut users = table(
        vec![column("select", "i64"), column("Email", "String")],
        &["select"],
    );
    users.schema = Some("Auth".to_owned());
    let rendered = render_alone(&users);
    assert!(rendered.contains("            \"select\",\n            \"Email\"\n        FROM \"Auth\".users\n        WHERE \"select\" = $1\n"));
}

#[test]
fn key_params_avoid_local_name_collisions() {
    let rendered = render_alone(&table(vec![column("limit", "i64")], &["limit"]));
    assert!(rendered.contains("get_by_id<'e, E>(executor: E, limit_key: i64)"));
    assert!(rendered.contains("WHERE \"limit\" = $1"));
    assert!(rendered.contains(".bind(limit_key)"));
}

#[test]
fn without_primary_key_only_unordered_list_and_count() {
    let rendered = render_alone(&table(vec![column("email", "String")], &[]));
    assert!(!rendered.contains("pub async fn get_by_id"));
    assert!(!rendered.contains("pub async fn exists_by_id"));
    assert!(!rendered.contains("pub async fn list_page"));
    assert!(!rendered.contains("pub async fn search_after"));
    let list = rendered.split("pub async fn list<").nth(1).unwrap();
    let list = list.split("pub async fn").next().unwrap();
    assert!(
        !list.contains("ORDER BY"),
        "list has no stable order to offer"
    );
    assert!(
        !rendered.contains("query.push(\", ")
            && rendered.contains("query.push(sort.direction.sql());"),
        "search sorts by the caller's key with no primary-key tie-break"
    );
    assert!(rendered.contains("No primary key detected"));
    assert!(rendered.contains("pub async fn list<'e, E>(executor: E)"));
    assert!(rendered.contains("pub async fn count<'e, E>(executor: E)"));
}

/// A text key is borrowed; `Copy` keys stay by value.
#[test]
fn text_keys_are_borrowed() {
    let mut code = column("code", "String");
    code.typed_id = Some(crate::schema::TypedId {
        module: "users".to_owned(),
        name: "UserId".to_owned(),
    });
    let rendered = render_alone(&table(vec![code, column("email", "String")], &["code"]));
    assert!(rendered.contains("get_by_id<'e, E>(executor: E, code: &UserId)"));
    assert!(rendered.contains("exists_by_id<'e, E>(executor: E, code: &UserId)"));
    assert!(
        rendered.contains("    after: Option<&UserId>,\n"),
        "{rendered}"
    );

    let rendered = render_alone(&table(
        vec![column("order_id", "uuid::Uuid"), column("sku", "String")],
        &["order_id", "sku"],
    ));
    assert!(
        rendered.contains("(executor: E, key: &UserKey)"),
        "{rendered}"
    );
    assert!(rendered.contains("    .bind(key.order_id)\n    .bind(&key.sku)\n"));
    assert!(rendered.contains("    after: Option<&UserKey>,\n"));
}

/// A `BYTEA` key is borrowed as a byte slice, like text as `&str`.
#[test]
fn byte_keys_are_borrowed_as_slices() {
    let rendered = render_alone(&table(
        vec![column("digest", "Vec<u8>"), column("email", "String")],
        &["digest"],
    ));
    assert!(rendered.contains("get_by_id<'e, E>(executor: E, digest: &[u8])"));
    assert!(
        rendered.contains("    after: Option<&[u8]>,\n"),
        "{rendered}"
    );
    assert!(rendered.contains(".bind(digest)"));
}

#[test]
fn output_is_deterministic() {
    let build = || render_alone(&table(vec![column("id", "i64")], &["id"]));
    assert_eq!(build(), build());
}
