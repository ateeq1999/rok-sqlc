//! `db/queries.rs` (one query file) or `db/queries/<stem>.rs` (one module per
//! query file): one typed function per annotated query.
//!
//! The SQL is embedded exactly as written in the query file and every
//! placeholder is bound positionally, so `$n` always receives the n-th
//! parameter.

use crate::queries::{AnalyzedQuery, Param, ResultShape, ReturnMode};
use crate::schema::{BaseType, Shape, Table, mentions_generated_type};

use super::{
    HEADER, QueryModule, StaticQuery, doc_name, push_derives, push_doc, push_fn_head,
    serde_derives, sql, use_paths,
};

/// Queries with more parameters than this take a `{Name}Params` struct
/// instead of positional arguments.
const MAX_POSITIONAL_PARAMS: usize = 3;

/// Whether the query's function takes a `{Name}Params` struct.
pub fn uses_params_struct(query: &AnalyzedQuery) -> bool {
    query.params.len() > MAX_POSITIONAL_PARAMS
}

/// Renders `db/queries.rs`, a direct child of `db`. `tables` are the
/// generated tables, whose typed ids the query types may name.
pub fn render(queries: &[AnalyzedQuery], tables: &[Table], serde: bool) -> String {
    render_at(queries, &TypedIds::of(tables), "super", serde)
}

/// Renders `db/queries/<stem>.rs`, one level below `db/queries.rs`.
pub fn render_nested(queries: &[AnalyzedQuery], tables: &[Table], serde: bool) -> String {
    render_at(queries, &TypedIds::of(tables), "super::super", serde)
}

/// The typed ids the generated tables define, which query parameters and
/// result columns drawn from a key column carry by name (`CategoryId`).
struct TypedIds<'a> {
    /// `(name, module, copy)`: the id, the resource defining it in its
    /// `models.rs`, and whether its value is `Copy`.
    ids: Vec<(&'a str, &'a str, bool)>,
}

impl<'a> TypedIds<'a> {
    fn of(tables: &'a [Table]) -> Self {
        let ids = tables
            .iter()
            .flat_map(|table| {
                table.columns.iter().filter_map(move |column| {
                    let id = column.typed_id.as_ref()?;
                    (id.module == table.module_name).then_some((
                        id.name.as_str(),
                        id.module.as_str(),
                        column.is_copy(),
                    ))
                })
            })
            .collect();
        Self { ids }
    }

    fn find(&self, name: &str) -> Option<&(&'a str, &'a str, bool)> {
        self.ids.iter().find(|(id, _, _)| *id == name)
    }

    /// The typed ids a Rust type names as a bare identifier (`CategoryId`,
    /// `Option<CategoryId>`, `Vec<CategoryId>`); a path such as
    /// `other::CategoryId` names something else.
    fn named_in<'t>(&'t self, rust_type: &'t str) -> impl Iterator<Item = (&'a str, &'a str)> + 't {
        rust_type
            .split(|character: char| !(character.is_alphanumeric() || "_:".contains(character)))
            .filter_map(|token| self.find(token).map(|(name, module, _)| (*name, *module)))
    }

    /// Whether `rust_type` is a typed id whose value is not `Copy` (a text
    /// key), which a parameter borrows.
    fn is_borrowed(&self, rust_type: &str) -> bool {
        self.find(rust_type).is_some_and(|(_, _, copy)| !copy)
    }
}

/// Renders `db/queries/mod.rs`, declaring one module per query file.
pub fn render_mod(modules: &[QueryModule]) -> String {
    // Sorted the way rustfmt sorts `mod` declarations.
    let mut names: Vec<&str> = modules.iter().map(|module| module.name.as_str()).collect();
    names.sort_unstable();
    let mut output = format!("{HEADER}\n//! One module per query file.\n\n");
    for name in names {
        output.push_str(&format!("pub mod {name};\n"));
    }
    output
}

/// `db_path` is the path from the rendered file to the `db` module; row
/// structs of generated resources and the `types` module are reached
/// through it.
fn render_at(queries: &[AnalyzedQuery], ids: &TypedIds<'_>, db_path: &str, serde: bool) -> String {
    let mut output =
        format!("{HEADER}\n//! Functions for the annotated queries in the query file.\n");
    let mentioned = mentioned_types(queries);
    let mut imports: Vec<String> = mentioned
        .iter()
        .flat_map(|rust_type| ids.named_in(rust_type))
        .map(|(name, module)| format!("{db_path}::{module}::models::{name}"))
        .collect();
    if mentioned
        .iter()
        .any(|rust_type| mentions_generated_type(rust_type))
    {
        imports.push(format!("{db_path}::types"));
    }
    if !imports.is_empty() {
        output.push_str(&format!("\n{}", use_paths(&imports)));
    }
    for query in queries {
        render_query(&mut output, query, ids, db_path, serde);
    }
    output
}

fn render_query(
    output: &mut String,
    query: &AnalyzedQuery,
    ids: &TypedIds<'_>,
    db_path: &str,
    serde: bool,
) {
    if let ResultShape::Row(columns) = &query.result {
        output.push('\n');
        push_doc(
            output,
            0,
            &[&format!("A row returned by `{}`.", query.fn_name)],
        );
        output.push_str("#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]\n");
        push_derives(output, serde_derives(serde));
        output.push_str(&format!("pub struct {}Row {{\n", query.name));
        for column in columns {
            push_doc(
                output,
                4,
                &[&format!("The {} column.", doc_name(&column.sql_name))],
            );
            if column.rust_name != column.sql_name {
                output.push_str(&format!(
                    "    #[sqlx(rename = {})]\n",
                    sql::str_literal(&column.sql_name)
                ));
            }
            output.push_str(&format!(
                "    pub {}: {},\n",
                column.rust_name, column.rust_type
            ));
        }
        output.push_str("}\n");
    }

    let uses_params_struct = uses_params_struct(query);
    if uses_params_struct {
        output.push('\n');
        push_doc(output, 0, &[&format!("Parameters of `{}`.", query.fn_name)]);
        output.push_str("#[derive(Debug, Clone, PartialEq)]\n");
        push_derives(output, serde_derives(serde));
        output.push_str(&format!("pub struct {}Params {{\n", query.name));
        for param in &query.params {
            push_doc(output, 4, &[&format!("Bound to `${}`.", param.index)]);
            output.push_str(&format!("    pub {}: {},\n", param.name, param.rust_type));
        }
        output.push_str("}\n");
    }

    let (fn_params, binds): (Vec<String>, Vec<String>) = if uses_params_struct {
        (
            vec![format!("params: {}Params", query.name)],
            query
                .params
                .iter()
                .map(|param| format!("params.{}", param.name))
                .collect(),
        )
    } else {
        (
            query
                .params
                .iter()
                .map(|param| signature_param(param, ids))
                .collect(),
            query
                .params
                .iter()
                .map(|param| param.name.clone())
                .collect(),
        )
    };

    let row_type = row_type(query, db_path);
    let (returns, execute) = match query.mode {
        ReturnMode::One => (row_type.clone(), "fetch_one(executor)"),
        ReturnMode::MaybeOne => (format!("Option<{row_type}>"), "fetch_optional(executor)"),
        ReturnMode::Many => (format!("Vec<{row_type}>"), "fetch_all(executor)"),
        ReturnMode::Exec => ("()".to_owned(), "execute(executor)"),
        ReturnMode::ExecRows => ("u64".to_owned(), "execute(executor)"),
    };

    output.push('\n');
    let mut doc = vec![format!(
        "Runs the `{}` query (`:{}`).",
        query.name,
        query.mode.keyword()
    )];
    match query.mode {
        ReturnMode::One => {
            doc.push("Fails with `sqlx::Error::RowNotFound` when no row matches.".to_owned())
        }
        ReturnMode::ExecRows => doc.push("Returns the number of affected rows.".to_owned()),
        _ => {}
    }
    let doc: Vec<&str> = doc.iter().map(String::as_str).collect();
    push_doc(output, 0, &doc);
    push_fn_head(
        output,
        &query.fn_name,
        &fn_params,
        &format!("Result<{returns}, sqlx::Error>"),
    );

    let call = match &query.result {
        ResultShape::None => "sqlx::query".to_owned(),
        ResultShape::Scalar(rust_type) => format!("sqlx::query_scalar::<_, {rust_type}>"),
        ResultShape::Model { .. } | ResultShape::Row(_) => {
            format!("sqlx::query_as::<_, {row_type}>")
        }
    };
    let statement = StaticQuery {
        call: &call,
        sql: query.sql.lines().map(str::to_owned).collect(),
        binds,
        execute,
    };
    match query.mode {
        ReturnMode::Exec => {
            statement.push(output, 4, "", "?;");
            output.push_str("    Ok(())\n");
        }
        ReturnMode::ExecRows => {
            statement.push(output, 4, "let result = ", "?;");
            output.push_str("    Ok(result.rows_affected())\n");
        }
        _ => statement.push(output, 4, "", ""),
    }
    output.push_str("}\n");
}

fn signature_param(param: &Param, ids: &TypedIds<'_>) -> String {
    format!("{}: {}", param.name, borrowed(&param.rust_type, ids))
}

/// A positional parameter's type: text, text typed ids, and lists are
/// borrowed (`&str`, `&CurrencyId`, `&[T]`, also inside `Option`), since
/// binding them needs no ownership.
fn borrowed(rust_type: &str, ids: &TypedIds<'_>) -> String {
    match Shape::of(rust_type) {
        Shape::Optional(inner) => format!("Option<{}>", borrowed(inner, ids)),
        Shape::List(item) => format!("&[{item}]"),
        Shape::Value(value) if BaseType::of(value) == BaseType::Text => "&str".to_owned(),
        Shape::Value(value) if ids.is_borrowed(value) => format!("&{value}"),
        Shape::Value(value) => value.to_owned(),
    }
}

fn row_type(query: &AnalyzedQuery, db_path: &str) -> String {
    match &query.result {
        ResultShape::None => "()".to_owned(),
        ResultShape::Model {
            module,
            struct_name,
        } => format!("{db_path}::{module}::models::{struct_name}"),
        ResultShape::Scalar(rust_type) => rust_type.clone(),
        ResultShape::Row(_) => format!("{}Row", query.name),
    }
}

/// Every Rust type the queries mention, for dependency hints.
pub fn mentioned_types<'a>(queries: impl IntoIterator<Item = &'a AnalyzedQuery>) -> Vec<String> {
    let mut types = Vec::new();
    for query in queries {
        types.extend(query.params.iter().map(|param| param.rust_type.clone()));
        match &query.result {
            ResultShape::Scalar(rust_type) => types.push(rust_type.clone()),
            ResultShape::Row(columns) => {
                types.extend(columns.iter().map(|column| column.rust_type.clone()));
            }
            ResultShape::None | ResultShape::Model { .. } => {}
        }
    }
    types
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queries::OutputColumn;

    fn render(queries: &[AnalyzedQuery]) -> String {
        super::render(queries, &[], false)
    }

    fn query(mode: ReturnMode, params: Vec<Param>, result: ResultShape) -> AnalyzedQuery {
        AnalyzedQuery {
            name: "ListThings".to_owned(),
            fn_name: "list_things".to_owned(),
            mode,
            sql: "SELECT id, label\nFROM things\nWHERE id = $1".to_owned(),
            params,
            result,
        }
    }

    fn param(index: usize, name: &str, rust_type: &str) -> Param {
        Param {
            index,
            name: name.to_owned(),
            rust_type: rust_type.to_owned(),
        }
    }

    #[test]
    fn renders_row_struct_and_positional_params() {
        let rendered = render(&[query(
            ReturnMode::Many,
            vec![param(1, "id", "i64")],
            ResultShape::Row(vec![
                OutputColumn {
                    sql_name: "id".to_owned(),
                    rust_name: "id".to_owned(),
                    rust_type: "i64".to_owned(),
                },
                OutputColumn {
                    sql_name: "type".to_owned(),
                    rust_name: "type_".to_owned(),
                    rust_type: "Option<String>".to_owned(),
                },
            ]),
        )]);
        assert!(rendered.contains(
            "#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]\npub struct ListThingsRow {\n    /// The `id` column.\n    pub id: i64,\n    /// The `type` column.\n    #[sqlx(rename = \"type\")]\n    pub type_: Option<String>,\n}\n"
        ));
        assert!(rendered.contains(
            "pub async fn list_things<'e, E>(executor: E, id: i64) -> Result<Vec<ListThingsRow>, sqlx::Error>"
        ));
        assert!(rendered.contains(
            "    sqlx::query_as::<_, ListThingsRow>(\n        r#\"\n        SELECT id, label\n        FROM things\n        WHERE id = $1\n        \"#,\n    )\n    .bind(id)\n    .fetch_all(executor)\n    .await\n}\n"
        ));
    }

    /// Text and list parameters are borrowed: binding them needs no
    /// ownership. A params struct is an input and stays owned.
    #[test]
    fn text_and_list_params_are_borrowed() {
        let rendered = render(&[query(
            ReturnMode::Exec,
            vec![
                param(1, "name", "String"),
                param(2, "note", "Option<String>"),
                param(3, "ids", "Vec<i64>"),
            ],
            ResultShape::None,
        )]);
        assert!(
            rendered.contains("    name: &str,\n    note: Option<&str>,\n    ids: &[i64],\n"),
            "{rendered}"
        );

        let params = (1..=4)
            .map(|index| param(index, &format!("p{index}"), "String"))
            .collect();
        let rendered = render(&[query(ReturnMode::Exec, params, ResultShape::None)]);
        assert!(rendered.contains("    pub p1: String,\n"));
    }

    #[test]
    fn serde_derives_query_rows_and_params_on_request() {
        let row = ResultShape::Row(vec![OutputColumn {
            sql_name: "id".to_owned(),
            rust_name: "id".to_owned(),
            rust_type: "i64".to_owned(),
        }]);
        let params: Vec<Param> = (1..=4)
            .map(|index| param(index, &format!("p{index}"), "i32"))
            .collect();
        let queries = [query(ReturnMode::Many, params, row)];
        assert!(!render(&queries).contains("serde"));
        let rendered = super::render(&queries, &[], true);
        let serde = "serde::Serialize, serde::Deserialize)]\n";
        assert!(rendered.contains(&format!("{serde}pub struct ListThingsRow {{")));
        assert!(rendered.contains(&format!("{serde}pub struct ListThingsParams {{")));
    }

    #[test]
    fn many_params_use_a_params_struct() {
        let params = (1..=4)
            .map(|index| param(index, &format!("p{index}"), "i32"))
            .collect();
        let rendered = render(&[query(ReturnMode::ExecRows, params, ResultShape::None)]);
        assert!(
            rendered.contains(
                "pub struct ListThingsParams {\n    /// Bound to `$1`.\n    pub p1: i32,"
            )
        );
        assert!(
            rendered
                .contains("(executor: E, params: ListThingsParams) -> Result<u64, sqlx::Error>")
        );
        assert!(rendered.contains("    .bind(params.p1)\n    .bind(params.p2)\n"));
        assert!(rendered.contains("    let result = sqlx::query(\n"));
        assert!(rendered.contains("    .await?;\n    Ok(result.rows_affected())\n"));
    }

    #[test]
    fn modes_choose_fetch_methods_and_return_types() {
        let model = ResultShape::Model {
            module: "things".to_owned(),
            struct_name: "Thing".to_owned(),
        };
        let one = render(&[query(ReturnMode::One, vec![], model.clone())]);
        assert!(one.contains("-> Result<super::things::models::Thing, sqlx::Error>"));
        assert!(one.contains("sqlx::query_as::<_, super::things::models::Thing>("));
        assert!(one.contains(".fetch_one(executor)"));
        assert!(one.contains("`sqlx::Error::RowNotFound`"));

        let maybe = render(&[query(
            ReturnMode::MaybeOne,
            vec![],
            ResultShape::Scalar("i64".to_owned()),
        )]);
        assert!(maybe.contains("-> Result<Option<i64>, sqlx::Error>"));
        assert!(maybe.contains("sqlx::query_scalar::<_, i64>("));
        assert!(maybe.contains(".fetch_optional(executor)"));

        let exec = render(&[query(ReturnMode::Exec, vec![], ResultShape::None)]);
        assert!(exec.contains("-> Result<(), sqlx::Error>"));
        assert!(exec.contains("    .execute(executor)\n    .await?;\n    Ok(())\n}"));
    }
}
