use crate::schema::Table;

use super::sql::{column_clause, key_order, key_predicate, table_ref};
use super::{HEADER, StaticQuery, key_params, param_name, push_doc, push_fn_head};

pub fn render(table: &Table) -> String {
    let mut output = format!("{HEADER}\nuse super::models::{};\n", table.struct_name);

    if table.primary_key.is_empty() {
        output.push_str(
            "\n// No primary key detected; get_by_id, exists_by_id, and list_page are not generated\n// for this table, and list returns rows in no particular order.\n",
        );
    } else {
        render_get_by_id(&mut output, table);
        render_exists_by_id(&mut output, table);
    }
    render_list(&mut output, table);
    if !table.primary_key.is_empty() {
        render_list_page(&mut output, table);
    }
    render_count(&mut output, table);

    output
}

fn render_get_by_id(output: &mut String, table: &Table) {
    let keys = table.primary_key_columns();
    let mut sql = column_clause("SELECT", table);
    sql.push(format!("FROM {}", table_ref(table)));
    sql.push(format!("WHERE {}", key_predicate(&keys, 1)));

    output.push('\n');
    push_doc(
        output,
        0,
        &["Fetches the row with the given primary key, or `None` if there is none."],
    );
    push_fn_head(
        output,
        "get_by_id",
        &key_params(&keys),
        &format!("Result<Option<{}>, sqlx::Error>", table.struct_name),
    );
    StaticQuery {
        call: &format!("sqlx::query_as::<_, {}>", table.struct_name),
        sql,
        binds: keys.iter().map(|column| param_name(column)).collect(),
        execute: "fetch_optional(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

fn render_exists_by_id(output: &mut String, table: &Table) {
    let keys = table.primary_key_columns();
    let sql = vec![
        "SELECT EXISTS (".to_owned(),
        "    SELECT 1".to_owned(),
        format!("    FROM {}", table_ref(table)),
        format!("    WHERE {}", key_predicate(&keys, 1)),
        ")".to_owned(),
    ];

    output.push('\n');
    push_doc(
        output,
        0,
        &["Returns whether a row with the given primary key exists."],
    );
    push_fn_head(
        output,
        "exists_by_id",
        &key_params(&keys),
        "Result<bool, sqlx::Error>",
    );
    StaticQuery {
        call: "sqlx::query_scalar::<_, bool>",
        sql,
        binds: keys.iter().map(|column| param_name(column)).collect(),
        execute: "fetch_one(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

fn render_list(output: &mut String, table: &Table) {
    let keys = table.primary_key_columns();
    let mut sql = column_clause("SELECT", table);
    sql.push(format!("FROM {}", table_ref(table)));
    let doc = if keys.is_empty() {
        "Fetches every row, in no particular order."
    } else {
        sql.push(format!("ORDER BY {}", key_order(&keys)));
        "Fetches every row, ordered by primary key."
    };

    output.push('\n');
    push_doc(output, 0, &[doc]);
    push_fn_head(
        output,
        "list",
        &[],
        &format!("Result<Vec<{}>, sqlx::Error>", table.struct_name),
    );
    StaticQuery {
        call: &format!("sqlx::query_as::<_, {}>", table.struct_name),
        sql,
        binds: Vec::new(),
        execute: "fetch_all(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

fn render_list_page(output: &mut String, table: &Table) {
    let keys = table.primary_key_columns();
    let mut sql = column_clause("SELECT", table);
    sql.push(format!("FROM {}", table_ref(table)));
    sql.push(format!("ORDER BY {}", key_order(&keys)));
    sql.push("LIMIT $1".to_owned());
    sql.push("OFFSET $2".to_owned());

    output.push('\n');
    push_doc(
        output,
        0,
        &[
            "Fetches at most `limit` rows after skipping `offset`, ordered by primary key so",
            "consecutive pages neither repeat nor skip rows. Both values must be non-negative.",
        ],
    );
    push_fn_head(
        output,
        "list_page",
        &["limit: i64".to_owned(), "offset: i64".to_owned()],
        &format!("Result<Vec<{}>, sqlx::Error>", table.struct_name),
    );
    StaticQuery {
        call: &format!("sqlx::query_as::<_, {}>", table.struct_name),
        sql,
        binds: vec!["limit".to_owned(), "offset".to_owned()],
        execute: "fetch_all(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

fn render_count(output: &mut String, table: &Table) {
    output.push('\n');
    push_doc(output, 0, &["Counts every row."]);
    push_fn_head(output, "count", &[], "Result<i64, sqlx::Error>");
    StaticQuery {
        call: "sqlx::query_scalar::<_, i64>",
        sql: vec![format!("SELECT COUNT(*) FROM {}", table_ref(table))],
        binds: Vec::new(),
        execute: "fetch_one(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{column, table};
    use super::*;

    #[test]
    fn renders_get_by_id_with_explicit_columns() {
        let rendered = render(&table(
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
        let rendered = render(&table(
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
        let rendered = render(&table(
            vec![
                column("order_id", "uuid::Uuid"),
                column("product_id", "uuid::Uuid"),
                column("quantity", "i32"),
            ],
            &["order_id", "product_id"],
        ));
        assert!(rendered.contains(
            "pub async fn get_by_id<'e, E>(\n    executor: E,\n    order_id: uuid::Uuid,\n    product_id: uuid::Uuid,\n) -> Result<Option<User>, sqlx::Error>"
        ));
        assert!(rendered.contains("WHERE order_id = $1 AND product_id = $2"));
        assert!(rendered.contains("    .bind(order_id)\n    .bind(product_id)\n"));
        assert!(rendered.contains("ORDER BY order_id, product_id"));
    }

    #[test]
    fn quotes_reserved_and_mixed_case_identifiers_and_keeps_schema() {
        let mut users = table(
            vec![column("select", "i64"), column("Email", "String")],
            &["select"],
        );
        users.schema = Some("Auth".to_owned());
        let rendered = render(&users);
        assert!(rendered.contains("            \"select\",\n            \"Email\"\n        FROM \"Auth\".users\n        WHERE \"select\" = $1\n"));
    }

    #[test]
    fn key_params_avoid_local_name_collisions() {
        let rendered = render(&table(vec![column("limit", "i64")], &["limit"]));
        assert!(rendered.contains("get_by_id<'e, E>(executor: E, limit_key: i64)"));
        assert!(rendered.contains("WHERE \"limit\" = $1"));
        assert!(rendered.contains(".bind(limit_key)"));
    }

    #[test]
    fn without_primary_key_only_unordered_list_and_count() {
        let rendered = render(&table(vec![column("email", "String")], &[]));
        assert!(!rendered.contains("pub async fn get_by_id"));
        assert!(!rendered.contains("pub async fn exists_by_id"));
        assert!(!rendered.contains("pub async fn list_page"));
        assert!(!rendered.contains("ORDER BY"));
        assert!(rendered.contains("No primary key detected"));
        assert!(rendered.contains("pub async fn list<'e, E>(executor: E)"));
        assert!(rendered.contains("pub async fn count<'e, E>(executor: E)"));
    }

    #[test]
    fn output_is_deterministic() {
        let build = || render(&table(vec![column("id", "i64")], &["id"]));
        assert_eq!(build(), build());
    }
}
