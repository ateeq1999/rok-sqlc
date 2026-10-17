//! Parser tests, grouped by behaviour.

use super::*;

mod alter_column;
mod alter_table;
mod check_rules;
mod create_table;
mod enums;
mod recovered;
mod value_lists;

fn parse(sql: &str) -> ParsedSchema {
    parse_schema(sql).expect("schema should parse")
}

fn one_table(sql: &str) -> Table {
    let parsed = parse(sql);
    assert!(
        parsed.failures.is_empty(),
        "unexpected failures: {:?}",
        parsed.failures
    );
    assert_eq!(parsed.schema.tables.len(), 1);
    parsed.schema.tables[0].clone()
}

fn parse_files(files: &[(&str, &str)]) -> ParsedSchema {
    let sources: Vec<SchemaSource<'_>> = files
        .iter()
        .map(|(path, sql)| SchemaSource {
            path: Path::new(path),
            sql,
        })
        .collect();
    parse_sources(&sources).expect("schema should parse")
}

fn column_names(table: &Table) -> Vec<&str> {
    table
        .columns
        .iter()
        .map(|column| column.sql_name.as_str())
        .collect()
}

fn enum_summary(schema: &Schema) -> Vec<(String, Option<&str>, Vec<&str>)> {
    schema
        .enums
        .iter()
        .map(|enum_type| {
            (
                enum_type.rust_name.clone(),
                enum_type.column.as_deref(),
                enum_type
                    .variants
                    .iter()
                    .map(|variant| variant.label.as_str())
                    .collect(),
            )
        })
        .collect()
}
