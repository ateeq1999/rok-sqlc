//! `create`, `update`, and `delete`.
//!
//! Statements whose shape is fixed at generation time are emitted as static
//! SQL. Statements whose shape depends on which optional fields are present
//! (an insert with defaulted columns, a partial update) are built at runtime
//! with `sqlx::QueryBuilder`, whose `push_bind` allocates each placeholder as
//! it binds the value, so placeholder numbers and bind order cannot drift
//! apart. Only identifiers from the schema reach the SQL text.

use crate::schema::{Column, Table};

use super::sql::{column_clause, ident, key_predicate, str_literal, table_ref};
use super::{
    HEADER, MAX_WIDTH, StaticQuery, key_params, param_name, push_and_chain, push_doc, push_fn_head,
    push_let_call,
};

pub fn render(table: &Table) -> String {
    let insert_columns = table.insert_columns();
    let update_columns = table.update_columns();
    let keys = table.primary_key_columns();
    let has_update = !keys.is_empty() && !update_columns.is_empty();

    let mut imports = vec![table.struct_name.clone()];
    if !insert_columns.is_empty() {
        imports.push(format!("New{}", table.struct_name));
    }
    if has_update {
        imports.push(format!("Update{}", table.struct_name));
    }
    imports.sort();
    let mut output = format!("{HEADER}\n{}", use_models(&imports));

    if insert_columns.is_empty() {
        output.push_str(
            "\n// No insertable columns detected; create is not generated for this table.\n",
        );
    } else if insert_columns
        .iter()
        .any(|column| column.is_omittable_on_insert())
    {
        render_dynamic_create(&mut output, table, &insert_columns);
    } else {
        render_static_create(&mut output, table, &insert_columns);
    }

    if keys.is_empty() {
        output.push_str(
            "\n// No primary key detected; update and delete are not generated for this table.\n",
        );
        return output;
    }

    if has_update {
        render_update(&mut output, table, &keys, &update_columns);
    } else {
        output.push_str(
            "\n// No updatable columns detected; update is not generated for this table.\n",
        );
    }
    render_delete(&mut output, table, &keys);

    output
}

/// `use super::models::...;` for sorted `names`, wrapped like rustfmt wraps a
/// use list that does not fit on one line.
fn use_models(names: &[String]) -> String {
    if let [name] = names {
        return format!("use super::models::{name};\n");
    }
    let one_line = format!("use super::models::{{{}}};", names.join(", "));
    if one_line.len() <= MAX_WIDTH {
        return format!("{one_line}\n");
    }
    let inner = format!("    {},", names.join(", "));
    if inner.len() <= MAX_WIDTH {
        format!("use super::models::{{\n{inner}\n}};\n")
    } else {
        let lines: String = names.iter().map(|name| format!("    {name},\n")).collect();
        format!("use super::models::{{\n{lines}}};\n")
    }
}

fn returning_clause(table: &Table) -> Vec<String> {
    column_clause("RETURNING", table)
}

fn push_create_head(output: &mut String, table: &Table, doc: &[&str]) {
    output.push('\n');
    push_doc(output, 0, doc);
    push_fn_head(
        output,
        "create",
        &[format!("input: New{}", table.struct_name)],
        &format!("Result<{}, sqlx::Error>", table.struct_name),
    );
}

fn render_static_create(output: &mut String, table: &Table, columns: &[&Column]) {
    let names: Vec<String> = columns
        .iter()
        .map(|column| ident(&column.sql_name))
        .collect();
    let placeholders: Vec<String> = (1..=columns.len())
        .map(|index| format!("${index}"))
        .collect();
    let mut sql = vec![
        format!("INSERT INTO {} ({})", table_ref(table), names.join(", ")),
        format!("VALUES ({})", placeholders.join(", ")),
    ];
    sql.extend(returning_clause(table));

    push_create_head(
        output,
        table,
        &["Inserts one row and returns it as stored."],
    );
    StaticQuery {
        call: &format!("sqlx::query_as::<_, {}>", table.struct_name),
        sql,
        binds: columns
            .iter()
            .map(|column| format!("input.{}", column.rust_name))
            .collect(),
        execute: "fetch_one(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

fn render_dynamic_create(output: &mut String, table: &Table, columns: &[&Column]) {
    push_create_head(
        output,
        table,
        &[
            "Inserts one row and returns it as stored. Defaulted columns left as `None`",
            "are omitted from the `INSERT` so the database default applies.",
        ],
    );

    // When every insertable column is defaulted, an all-`None` input lists no
    // columns at all, which only `DEFAULT VALUES` can express.
    if columns.iter().all(|column| column.is_omittable_on_insert()) {
        let terms: Vec<String> = columns
            .iter()
            .map(|column| format!("input.{}.is_none()", column.rust_name))
            .collect();
        push_and_chain(output, 4, "if ", &terms, " {");
        let mut sql = vec![
            format!("INSERT INTO {}", table_ref(table)),
            "DEFAULT VALUES".to_owned(),
        ];
        sql.extend(returning_clause(table));
        StaticQuery {
            call: &format!("sqlx::query_as::<_, {}>", table.struct_name),
            sql,
            binds: Vec::new(),
            execute: "fetch_one(executor)",
        }
        .push(output, 8, "return ", ";");
        output.push_str("    }\n\n");
    }

    push_let_call(
        output,
        4,
        "let mut query",
        "sqlx::QueryBuilder::<sqlx::Postgres>::new",
        &str_literal(&format!("INSERT INTO {} (", table_ref(table))),
    );
    output.push_str("    let mut columns = query.separated(\", \");\n");
    for column in columns {
        let name = str_literal(&ident(&column.sql_name));
        if column.is_omittable_on_insert() {
            output.push_str(&format!("    if input.{}.is_some() {{\n", column.rust_name));
            output.push_str(&format!("        columns.push({name});\n"));
            output.push_str("    }\n");
        } else {
            output.push_str(&format!("    columns.push({name});\n"));
        }
    }
    output.push_str("    query.push(\") VALUES (\");\n");
    output.push_str("    let mut values = query.separated(\", \");\n");
    for column in columns {
        if column.is_omittable_on_insert() {
            output.push_str(&format!(
                "    if let Some(value) = input.{} {{\n",
                column.rust_name
            ));
            output.push_str("        values.push_bind(value);\n");
            output.push_str("    }\n");
        } else {
            output.push_str(&format!(
                "    values.push_bind(input.{});\n",
                column.rust_name
            ));
        }
    }
    output.push_str("    query.push(\")\");\n");
    push_returning(output, table);
    push_build_and_fetch(output, table, "fetch_one");
    output.push_str("}\n");
}

fn render_update(output: &mut String, table: &Table, keys: &[&Column], columns: &[&Column]) {
    let mut params = key_params(keys);
    params.push(format!("input: Update{}", table.struct_name));

    output.push('\n');
    push_doc(
        output,
        0,
        &[
            "Applies a partial update and returns the updated row, or `None` if no row has",
            "the given primary key. An empty patch issues no `UPDATE` and returns the",
            "current row.",
        ],
    );
    push_fn_head(
        output,
        "update",
        &params,
        &format!("Result<Option<{}>, sqlx::Error>", table.struct_name),
    );

    let key_arguments: Vec<String> = keys.iter().map(|column| param_name(column)).collect();
    output.push_str("    if input.is_empty() {\n");
    output.push_str(&format!(
        "        return super::read::get_by_id(executor, {}).await;\n",
        key_arguments.join(", ")
    ));
    output.push_str("    }\n\n");

    push_let_call(
        output,
        4,
        "let mut query",
        "sqlx::QueryBuilder::<sqlx::Postgres>::new",
        &str_literal(&format!("UPDATE {} SET ", table_ref(table))),
    );
    output.push_str("    let mut assignments = query.separated(\", \");\n");
    for column in columns {
        output.push_str(&format!(
            "    if let Some(value) = input.{} {{\n",
            column.rust_name
        ));
        output.push_str(&format!(
            "        assignments.push({});\n",
            str_literal(&format!("{} = ", ident(&column.sql_name)))
        ));
        output.push_str("        assignments.push_bind_unseparated(value);\n");
        output.push_str("    }\n");
    }
    for (index, column) in keys.iter().enumerate() {
        let connective = if index == 0 { " WHERE" } else { " AND" };
        output.push_str(&format!(
            "    query.push({});\n",
            str_literal(&format!("{connective} {} = ", ident(&column.sql_name)))
        ));
        output.push_str(&format!("    query.push_bind({});\n", param_name(column)));
    }
    push_returning(output, table);
    push_build_and_fetch(output, table, "fetch_optional");
    output.push_str("}\n");
}

fn render_delete(output: &mut String, table: &Table, keys: &[&Column]) {
    output.push('\n');
    push_doc(
        output,
        0,
        &["Deletes the row with the given primary key and returns whether one existed."],
    );
    push_fn_head(
        output,
        "delete",
        &key_params(keys),
        "Result<bool, sqlx::Error>",
    );
    StaticQuery {
        call: "sqlx::query",
        sql: vec![
            format!("DELETE FROM {}", table_ref(table)),
            format!("WHERE {}", key_predicate(keys, 1)),
        ],
        binds: keys.iter().map(|column| param_name(column)).collect(),
        execute: "execute(executor)",
    }
    .push(output, 4, "let result = ", "?;");
    output.push_str("    Ok(result.rows_affected() > 0)\n");
    output.push_str("}\n");
}

fn push_returning(output: &mut String, table: &Table) {
    output.push_str("    query.push(\n");
    output.push_str(&super::sql::raw_block(&returning_clause(table), 8));
    output.push_str(",\n    );\n");
}

/// The final `build_query_as().fetch_*().await` chain, kept on one line when
/// it fits rustfmt's default `chain_width` of 60 and split per call otherwise.
pub(super) fn push_build_and_fetch(output: &mut String, table: &Table, fetch: &str) {
    let calls = [
        format!(".build_query_as::<{}>()", table.struct_name),
        format!(".{fetch}(executor)"),
        ".await".to_owned(),
    ];
    let one_line = format!("query{}", calls.concat());
    if one_line.len() <= 60 {
        output.push_str(&format!("    {one_line}\n"));
    } else {
        output.push_str("    query\n");
        for call in calls {
            output.push_str(&format!("        {call}\n"));
        }
    }
}

#[cfg(test)]
mod tests {
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
        assert!(
            rendered.contains("    query.build_query_as::<User>().fetch_one(executor).await\n")
        );
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
            "pub async fn update<'e, E>(\n    executor: E,\n    order_id: uuid::Uuid,\n    product_id: uuid::Uuid,\n    input: UpdateUser,\n) -> Result<Option<User>, sqlx::Error>"
        ));
        assert!(rendered.contains("    if input.is_empty() {\n        return super::read::get_by_id(executor, order_id, product_id).await;\n    }\n"));
        assert!(rendered.contains("    if let Some(value) = input.quantity {\n        assignments.push(\"quantity = \");\n        assignments.push_bind_unseparated(value);\n    }\n"));
        assert!(rendered.contains("    query.push(\" WHERE order_id = \");\n    query.push_bind(order_id);\n    query.push(\" AND product_id = \");\n    query.push_bind(product_id);\n"));
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
        assert!(rendered.contains(
            "pub async fn delete<'e, E>(executor: E, id: i64) -> Result<bool, sqlx::Error>"
        ));
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
}
