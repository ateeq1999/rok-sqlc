//! `delete`, and `restore` and `delete_permanently` under soft delete.

use crate::schema::{Column, Table};

use crate::generator::sql::{ident, scoped_key_predicate, table_ref};
use crate::generator::{StaticQuery, keys, push_doc, push_fn_head};

pub(super) fn render_delete(output: &mut String, table: &Table, key_columns: &[&Column]) {
    let Some(column) = table.soft_delete_column.as_deref() else {
        render_key_statement(
            output,
            table,
            "delete",
            &["Deletes the row with the given primary key and returns whether one existed."],
            vec![
                format!("DELETE FROM {}", table_ref(table)),
                format!("WHERE {}", scoped_key_predicate(table, key_columns)),
            ],
        );
        return;
    };
    let column = ident(column);
    let soft = |value: &str, condition: &str| {
        vec![
            format!("UPDATE {}", table_ref(table)),
            format!("SET {column} = {value}"),
            format!(
                "WHERE {} AND {column} {condition}",
                scoped_key_predicate(table, key_columns)
            ),
        ]
    };
    render_key_statement(
        output,
        table,
        "delete",
        &[
            &format!("Soft-deletes the row with the given primary key by setting `{column}` to"),
            "the current time, and returns whether a row changed: `false` when there is no",
            "such row or it is already deleted.",
        ],
        soft("now()", "IS NULL"),
    );
    render_key_statement(
        output,
        table,
        "restore",
        &[
            &format!(
                "Restores the soft-deleted row with the given primary key by clearing `{column}`,"
            ),
            "and returns whether a row changed: `false` when there is no such row or it is",
            "not deleted.",
        ],
        soft("NULL", "IS NOT NULL"),
    );
    render_key_statement(
        output,
        table,
        "delete_permanently",
        &[
            "Removes the row with the given primary key from the table, deleted or not, and",
            "returns whether one existed.",
        ],
        vec![
            format!("DELETE FROM {}", table_ref(table)),
            format!("WHERE {}", scoped_key_predicate(table, key_columns)),
        ],
    );
}

/// A function taking the primary key that runs `sql` and returns whether it
/// affected a row.
fn render_key_statement(
    output: &mut String,
    table: &Table,
    name: &str,
    doc: &[&str],
    sql: Vec<String>,
) {
    output.push('\n');
    push_doc(output, 0, doc);
    push_fn_head(
        output,
        name,
        &keys::key_params(table),
        "Result<bool, sqlx::Error>",
    );
    StaticQuery {
        call: "sqlx::query",
        sql,
        binds: keys::scoped_key_binds(table),
        execute: "execute(executor)",
    }
    .push(output, 4, "let result = ", "?;");
    output.push_str("    Ok(result.rows_affected() > 0)\n");
    output.push_str("}\n");
}
