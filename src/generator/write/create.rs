//! `create`: a static insert, or one built at runtime when columns have
//! defaults.

use crate::schema::{Column, Table};

use super::{push_returning, returning_clause};
use crate::generator::sql::{ident, str_literal, table_ref};
use crate::generator::{
    StaticQuery, insert_bind, keys, push_and_chain, push_build_and_fetch, push_doc, push_fn_head,
    push_let_call,
};

fn push_create_head(output: &mut String, table: &Table, doc: &[&str]) {
    output.push('\n');
    push_doc(output, 0, doc);
    push_fn_head(
        output,
        "create",
        &keys::scoped_params(table, &[format!("input: New{}", table.struct_name)]),
        &format!("Result<{}, sqlx::Error>", table.struct_name),
    );
}

pub(super) fn render_static_create(output: &mut String, table: &Table, columns: &[&Column]) {
    let names: Vec<String> = table
        .tenant()
        .into_iter()
        .chain(columns.iter().copied())
        .map(|column| ident(&column.sql_name))
        .collect();
    let placeholders: Vec<String> = (1..=names.len()).map(|index| format!("${index}")).collect();
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
        binds: keys::tenant_bind(table)
            .into_iter()
            .chain(columns.iter().map(|column| insert_bind(column)))
            .collect(),
        execute: "fetch_one(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

pub(super) fn render_dynamic_create(output: &mut String, table: &Table, columns: &[&Column]) {
    push_create_head(
        output,
        table,
        &[
            "Inserts one row and returns it as stored. Defaulted columns left as `None`",
            "are omitted from the `INSERT` so the database default applies.",
        ],
    );

    // When every insertable column is defaulted, an all-`None` input lists no
    // columns at all, which only `DEFAULT VALUES` can express. A tenant table
    // always lists its tenant column.
    if table.tenant().is_none() && columns.iter().all(|column| column.is_omittable_on_insert()) {
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
    if let Some(tenant) = table.tenant() {
        output.push_str(&format!(
            "    columns.push({});\n",
            str_literal(&ident(&tenant.sql_name))
        ));
    }
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
    if let Some(tenant) = keys::tenant_bind(table) {
        output.push_str(&format!("    values.push_bind({tenant});\n"));
    }
    for column in columns {
        if column.is_omittable_on_insert() {
            output.push_str(&format!(
                "    if let Some(value) = input.{} {{\n",
                column.rust_name
            ));
            output.push_str("        values.push_bind(value);\n");
            output.push_str("    }\n");
        } else {
            output.push_str(&format!("    values.push_bind({});\n", insert_bind(column)));
        }
    }
    output.push_str("    query.push(\")\");\n");
    push_returning(output, table);
    push_build_and_fetch(output, table, "fetch_one");
    output.push_str("}\n");
}
