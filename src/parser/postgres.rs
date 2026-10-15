use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use sqlparser::ast::{
    ArrayElemTypeDef, ColumnOption, CreateTable, DataType, ExactNumberInfo, Expr, Ident,
    IndexColumn, ObjectName, ObjectNamePart, Spanned, Statement, TableConstraint, TimezoneInfo,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

use super::diagnostics::TableFailure;
use super::naming;
use crate::schema::{Column, ForeignKey, Schema, Table};

pub struct ParsedSchema {
    pub schema: Schema,
    pub failures: Vec<TableFailure>,
}

/// Root modules rok-sqlc always generates next to the resource modules.
const GENERATED_MODULES: &[&str] = &["schema", "search"];

/// One schema input: a migration file's path and its SQL.
pub struct SchemaSource<'a> {
    pub path: &'a Path,
    pub sql: &'a str,
}

/// Parses one schema text. Failures carry the line of their `CREATE TABLE`
/// but no path.
pub fn parse_schema(sql: &str) -> Result<ParsedSchema> {
    let mut parsed = parse_sources(&[SchemaSource {
        path: Path::new(""),
        sql,
    }])?;
    for failure in &mut parsed.failures {
        failure.path = None;
    }
    Ok(parsed)
}

/// Parses schema inputs in order, as one schema. Each file is parsed on its
/// own, so every failure names its file and the line of its `CREATE TABLE`
/// within that file.
pub fn parse_sources(sources: &[SchemaSource<'_>]) -> Result<ParsedSchema> {
    let dialect = PostgreSqlDialect {};
    let mut tables = Vec::new();
    let mut failures = Vec::new();
    let mut seen_modules: HashSet<String> = HashSet::new();

    for source in sources {
        let statements = Parser::parse_sql(&dialect, source.sql).with_context(|| {
            if source.path.as_os_str().is_empty() {
                "invalid PostgreSQL SQL syntax".to_owned()
            } else {
                format!(
                    "invalid PostgreSQL SQL syntax in `{}`",
                    source.path.display()
                )
            }
        })?;

        for statement in statements {
            let line = usize::try_from(statement.span().start.line).ok();
            let Statement::CreateTable(create) = statement else {
                continue;
            };
            let failure = match build_table(&create) {
                Ok(table) if GENERATED_MODULES.contains(&table.module_name.as_str()) => {
                    table_failure(
                        &table.sql_name,
                        format!(
                            "module name `{}` is reserved for the generated `db/{}.rs`",
                            table.module_name, table.module_name
                        ),
                    )
                }
                Ok(table) if seen_modules.insert(table.module_name.clone()) => {
                    tables.push(table);
                    continue;
                }
                Ok(table) => table_failure(
                    &table.sql_name,
                    format!(
                        "module name `{}` is already used by another table",
                        table.module_name
                    ),
                ),
                Err(failure) => failure,
            };
            failures.push(TableFailure {
                path: Some(source.path.to_path_buf()),
                line,
                ..failure
            });
        }
    }

    if tables.is_empty() && failures.is_empty() {
        bail!("no supported CREATE TABLE statements found");
    }

    Ok(ParsedSchema {
        schema: Schema { tables },
        failures,
    })
}

fn build_table(create: &CreateTable) -> std::result::Result<Table, TableFailure> {
    let sql_name = object_leaf_name(&create.name);
    let fail = |reason: String| table_failure(&sql_name, reason);

    if create.columns.is_empty() {
        return Err(fail("table has no columns".to_owned()));
    }

    let mut columns = Vec::new();
    let mut raw_columns: Vec<RawColumn> = Vec::new();
    let mut seen_fields: HashSet<String> = HashSet::new();
    let mut foreign_keys = Vec::new();
    let mut inline_primary_keys: Vec<String> = Vec::new();

    for column_def in &create.columns {
        let column_name = ident_name(&column_def.name);
        let column_fail =
            |reason: String| table_failure(&sql_name, format!("column `{column_name}`: {reason}"));

        let nullable = !column_def
            .options
            .iter()
            .any(|option| matches!(option.option, ColumnOption::NotNull));
        let inline_pk = column_def.options.iter().any(|option| {
            matches!(
                option.option,
                ColumnOption::Unique {
                    is_primary: true,
                    ..
                }
            )
        });

        let mut is_generated = is_serial_type(&column_def.data_type);
        let mut has_default = is_generated;
        for option in &column_def.options {
            if let ColumnOption::ForeignKey {
                foreign_table,
                referred_columns,
                ..
            } = &option.option
                && referred_columns.len() == 1
            {
                foreign_keys.push(ForeignKey {
                    column: column_name.clone(),
                    referenced_table: object_leaf_name(foreign_table),
                    referenced_column: ident_name(&referred_columns[0]),
                });
            }
            match &option.option {
                ColumnOption::Default(_) => has_default = true,
                ColumnOption::Generated { .. } | ColumnOption::Identity(_) => {
                    has_default = true;
                    is_generated = true;
                }
                _ => {}
            }
        }

        if inline_pk {
            inline_primary_keys.push(column_name.clone());
        }

        let (base_type, sqlx_type) = map_type(&column_def.data_type).map_err(column_fail)?;

        let rust_name = naming::rust_identifier(&column_name).map_err(column_fail)?;
        if !seen_fields.insert(rust_name.clone()) {
            return Err(table_failure(
                &sql_name,
                format!(
                    "column `{column_name}` maps to Rust field `{rust_name}`, which is already taken by another column"
                ),
            ));
        }

        raw_columns.push(RawColumn {
            sql_name: column_name,
            rust_name,
            base_type,
            sqlx_type,
            nullable,
            has_default,
            is_generated,
        });
    }

    if inline_primary_keys.len() > 1 {
        return Err(fail(format!(
            "multiple inline primary keys ({}) are not supported",
            inline_primary_keys.join(", ")
        )));
    }

    let mut table_primary_key: Vec<String> = Vec::new();
    for constraint in &create.constraints {
        match constraint {
            TableConstraint::PrimaryKey { columns: keys, .. } => {
                let mut names = Vec::with_capacity(keys.len());
                for key in keys {
                    match index_column_name(key) {
                        Some(name) => names.push(name),
                        None => {
                            return Err(fail(
                                "PRIMARY KEY constraint contains a non-column expression"
                                    .to_owned(),
                            ));
                        }
                    }
                }
                let mut seen_keys: HashSet<&str> = HashSet::new();
                for name in &names {
                    if !seen_keys.insert(name.as_str()) {
                        return Err(fail(format!(
                            "PRIMARY KEY lists column `{name}` more than once"
                        )));
                    }
                }
                table_primary_key = names;
            }
            TableConstraint::ForeignKey {
                columns,
                foreign_table,
                referred_columns,
                ..
            } if columns.len() == 1 && referred_columns.len() == 1 => {
                foreign_keys.push(ForeignKey {
                    column: ident_name(&columns[0]),
                    referenced_table: object_leaf_name(foreign_table),
                    referenced_column: ident_name(&referred_columns[0]),
                });
            }
            _ => {}
        }
    }

    let primary_key = if table_primary_key.is_empty() {
        inline_primary_keys
    } else {
        table_primary_key
    };

    for key in &primary_key {
        if !raw_columns.iter().any(|raw| raw.sql_name == *key) {
            return Err(fail(format!(
                "primary key `{key}` does not match any column"
            )));
        }
    }

    for mut raw in raw_columns {
        let is_primary_key = primary_key.contains(&raw.sql_name);
        if is_primary_key {
            raw.nullable = false;
        }
        columns.push(Column {
            sql_name: raw.sql_name,
            rust_name: raw.rust_name,
            rust_type: if raw.nullable {
                format!("Option<{}>", raw.base_type)
            } else {
                raw.base_type
            },
            sqlx_type: raw.sqlx_type,
            nullable: raw.nullable,
            is_primary_key,
            has_default: raw.has_default,
            is_generated: raw.is_generated,
        });
    }

    check_search_names(&columns).map_err(fail)?;
    let module_name = naming::rust_identifier(&sql_name).map_err(fail)?;
    let struct_name = naming::struct_name(&module_name);

    Ok(Table {
        schema: object_schema_name(&create.name),
        sql_name,
        module_name,
        struct_name,
        columns,
        primary_key,
        foreign_keys,
    })
}

/// Rejects tables whose generated search names would collide: a nullable
/// column's `x_is_null` filter field against another field, or two columns
/// mapping to the same `XSortKey` variant.
fn check_search_names(columns: &[Column]) -> std::result::Result<(), String> {
    let mut fields: HashMap<String, &str> = columns
        .iter()
        .map(|column| (column.rust_name.clone(), column.sql_name.as_str()))
        .collect();
    for column in columns.iter().filter(|column| column.nullable) {
        let field = column.null_check_field();
        if let Some(other) = fields.insert(field.clone(), &column.sql_name) {
            return Err(format!(
                "column `{}` needs the null-check filter field `{field}`, which column `{other}` already uses",
                column.sql_name
            ));
        }
    }

    let mut variants: HashMap<String, &str> = HashMap::new();
    for column in columns
        .iter()
        .filter(|column| column.filter_kind().is_some())
    {
        let variant = column.sort_variant();
        if let Some(other) = variants.insert(variant.clone(), &column.sql_name) {
            return Err(format!(
                "columns `{other}` and `{}` both map to the sort key variant `{variant}`",
                column.sql_name
            ));
        }
    }
    Ok(())
}

fn table_failure(table: &str, reason: String) -> TableFailure {
    TableFailure {
        table: table.to_owned(),
        reason,
        path: None,
        line: None,
    }
}

struct RawColumn {
    sql_name: String,
    rust_name: String,
    base_type: String,
    sqlx_type: String,
    nullable: bool,
    has_default: bool,
    is_generated: bool,
}

pub(crate) fn ident_name(ident: &Ident) -> String {
    match ident.quote_style {
        Some('"') => ident.value.clone(),
        _ => ident.value.to_lowercase(),
    }
}

pub(crate) fn object_leaf_name(name: &ObjectName) -> String {
    name.0
        .iter()
        .rev()
        .find_map(|part| match part {
            ObjectNamePart::Identifier(ident) => Some(ident_name(ident)),
            ObjectNamePart::Function(_) => None,
        })
        .unwrap_or_default()
}

/// The schema qualifier of a possibly qualified name (`catalog` in
/// `catalog.products`); `None` for an unqualified name.
fn object_schema_name(name: &ObjectName) -> Option<String> {
    let identifiers: Vec<&Ident> = name
        .0
        .iter()
        .filter_map(|part| match part {
            ObjectNamePart::Identifier(ident) => Some(ident),
            ObjectNamePart::Function(_) => None,
        })
        .collect();
    match identifiers.as_slice() {
        [.., schema, _table] => Some(ident_name(schema)),
        _ => None,
    }
}

fn index_column_name(column: &IndexColumn) -> Option<String> {
    match &column.column.expr {
        Expr::Identifier(ident) => Some(ident_name(ident)),
        _ => None,
    }
}

pub(crate) fn map_type(data_type: &DataType) -> std::result::Result<(String, String), String> {
    let base = match data_type {
        DataType::SmallInt(_) | DataType::Int2(_) => "i16",
        DataType::Int(_) | DataType::Int4(_) | DataType::Integer(_) => "i32",
        DataType::BigInt(_) | DataType::Int8(_) => "i64",
        DataType::Bool | DataType::Boolean => "bool",
        DataType::Real => "f32",
        DataType::Float8 | DataType::DoublePrecision | DataType::Double(_) | DataType::Float(_) => {
            "f64"
        }
        DataType::Character(_)
        | DataType::Char(_)
        | DataType::CharacterVarying(_)
        | DataType::Varchar(_)
        | DataType::Text
        | DataType::String(_) => "String",
        DataType::Uuid => "uuid::Uuid",
        DataType::Date => "chrono::NaiveDate",
        DataType::Time(_, TimezoneInfo::None | TimezoneInfo::WithoutTimeZone) => {
            "chrono::NaiveTime"
        }
        DataType::Timestamp(_, TimezoneInfo::None | TimezoneInfo::WithoutTimeZone) => {
            "chrono::NaiveDateTime"
        }
        DataType::Timestamp(_, TimezoneInfo::Tz | TimezoneInfo::WithTimeZone) => {
            "chrono::DateTime<chrono::Utc>"
        }
        DataType::JSON | DataType::JSONB => "serde_json::Value",
        DataType::Numeric(info) | DataType::Decimal(info) => {
            check_decimal_precision(info)?;
            "rust_decimal::Decimal"
        }
        DataType::Array(element) => {
            let inner = array_element_type(element)?;
            let (base, _) = map_type(inner)?;
            return Ok((format!("Vec<{base}>"), format!("Vec<{base}>")));
        }
        DataType::Custom(name, _) => custom_type_leaf(name)?,
        other => return Err(format!("type `{other}` is not supported yet")),
    };

    Ok((base.to_owned(), base.to_owned()))
}

fn custom_type_leaf(name: &ObjectName) -> std::result::Result<&'static str, String> {
    match object_leaf_name(name).to_ascii_uppercase().as_str() {
        "SERIAL" => Ok("i32"),
        "BIGSERIAL" => Ok("i64"),
        "SMALLSERIAL" => Ok("i16"),
        "FLOAT4" => Ok("f32"),
        other => Err(format!("type `{other}` is not supported yet")),
    }
}

fn is_serial_type(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::Custom(name, _)
            if matches!(
                object_leaf_name(name).to_ascii_uppercase().as_str(),
                "SERIAL" | "BIGSERIAL" | "SMALLSERIAL"
            )
    )
}

fn array_element_type(element: &ArrayElemTypeDef) -> std::result::Result<&DataType, String> {
    match element {
        ArrayElemTypeDef::SquareBracket(inner, _)
        | ArrayElemTypeDef::AngleBracket(inner)
        | ArrayElemTypeDef::Parenthesis(inner) => Ok(inner),
        ArrayElemTypeDef::None => Err("array type has no element type".to_owned()),
    }
}

const DECIMAL_MAX_PRECISION: u64 = 28;

fn check_decimal_precision(info: &ExactNumberInfo) -> std::result::Result<(), String> {
    let precision = match info {
        ExactNumberInfo::None => return Ok(()),
        ExactNumberInfo::Precision(precision)
        | ExactNumberInfo::PrecisionAndScale(precision, _) => *precision,
    };
    if precision > DECIMAL_MAX_PRECISION {
        return Err(format!(
            "NUMERIC precision {precision} exceeds rust_decimal's {DECIMAL_MAX_PRECISION} significant digits"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn parses_simple_table() {
        let table = one_table("CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);");
        assert_eq!(table.sql_name, "users");
        assert_eq!(table.primary_key, ["id"]);
        assert!(table.columns[0].is_primary_key);
        assert!(!table.columns[1].nullable);
    }

    #[test]
    fn names_modules_and_structs_mechanically() {
        let table = one_table("CREATE TABLE user_profiles (id BIGSERIAL PRIMARY KEY);");
        assert_eq!(table.module_name, "user_profiles");
        assert_eq!(table.struct_name, "UserProfile");
    }

    #[test]
    fn detects_table_level_primary_key() {
        let table = one_table("CREATE TABLE items (a INT NOT NULL, PRIMARY KEY (a));");
        assert_eq!(table.primary_key, ["a"]);
    }

    #[test]
    fn accepts_composite_primary_key_in_declaration_order() {
        let table = one_table(
            "CREATE TABLE order_items (a INT NOT NULL, b INT NOT NULL, c TEXT, PRIMARY KEY (a, b));",
        );
        assert_eq!(table.primary_key, ["a", "b"]);
        assert!(table.columns[0].is_primary_key);
        assert!(table.columns[1].is_primary_key);
        assert!(!table.columns[2].is_primary_key);
        assert!(!table.columns[0].nullable);
        assert!(!table.columns[1].nullable);
    }

    #[test]
    fn rejects_primary_key_listing_unknown_column() {
        let parsed = parse("CREATE TABLE items (a INT, b INT, PRIMARY KEY (a, c));");
        assert_eq!(parsed.schema.tables.len(), 0);
        assert!(
            parsed.failures[0]
                .reason
                .contains("does not match any column")
        );
    }

    #[test]
    fn rejects_primary_key_listing_duplicate_column() {
        let parsed = parse("CREATE TABLE items (a INT, PRIMARY KEY (a, a));");
        assert_eq!(parsed.schema.tables.len(), 0);
        assert!(parsed.failures[0].reason.contains("more than once"));
    }

    #[test]
    fn rejects_unsupported_type() {
        let parsed =
            parse("CREATE TABLE shifts (id BIGSERIAL PRIMARY KEY, span INTERVAL NOT NULL);");
        assert_eq!(parsed.schema.tables.len(), 0);
        assert!(parsed.failures[0].reason.contains("not supported"));
    }

    #[test]
    fn maps_numeric_and_decimal_columns() {
        let table = one_table(
            "CREATE TABLE money (amount NUMERIC(12, 2) NOT NULL, fee DECIMAL, rate NUMERIC(6,3));",
        );
        assert_eq!(table.columns[0].rust_type, "rust_decimal::Decimal");
        assert_eq!(table.columns[1].rust_type, "Option<rust_decimal::Decimal>");
        assert_eq!(table.columns[2].rust_type, "Option<rust_decimal::Decimal>");
    }

    #[test]
    fn rejects_decimal_precision_beyond_rust_decimal() {
        let parsed = parse("CREATE TABLE money (amount NUMERIC(50, 10) NOT NULL);");
        assert_eq!(parsed.schema.tables.len(), 0);
        assert!(parsed.failures[0].reason.contains("exceeds"));
    }

    #[test]
    fn folds_unquoted_case_and_keeps_quoted() {
        let table =
            one_table("CREATE TABLE \"Users\" (ID UUID PRIMARY KEY, \"Email\" TEXT NOT NULL);");
        assert_eq!(table.sql_name, "Users");
        assert_eq!(table.columns[0].sql_name, "id");
        assert_eq!(table.columns[1].sql_name, "Email");
    }

    #[test]
    fn keeps_schema_qualification() {
        let table = one_table("CREATE TABLE public.users (id BIGSERIAL PRIMARY KEY);");
        assert_eq!(table.sql_name, "users");
        assert_eq!(table.schema.as_deref(), Some("public"));
        assert_eq!(table.display_name(), "public.users");
    }

    #[test]
    fn maps_serials_and_timestamps() {
        let table = one_table(
            "CREATE TABLE events (a SERIAL PRIMARY KEY, b BIGSERIAL, c TIMESTAMPTZ NOT NULL, d TIMESTAMP, e VARCHAR(255));",
        );
        assert_eq!(table.columns[0].sqlx_type, "i32");
        assert_eq!(table.columns[1].sqlx_type, "i64");
        assert_eq!(table.columns[2].rust_type, "chrono::DateTime<chrono::Utc>");
        assert_eq!(table.columns[3].rust_type, "Option<chrono::NaiveDateTime>");
        assert_eq!(table.columns[4].rust_type, "Option<String>");
    }

    #[test]
    fn flags_serial_columns_as_generated_with_default() {
        let table = one_table("CREATE TABLE events (id BIGSERIAL PRIMARY KEY, note TEXT);");
        assert!(table.columns[0].is_generated);
        assert!(table.columns[0].has_default);
        assert!(!table.columns[1].is_generated);
        assert!(!table.columns[1].has_default);
    }

    #[test]
    fn flags_default_clause_columns() {
        let table = one_table(
            "CREATE TABLE events (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), note TEXT);",
        );
        assert!(table.columns[1].has_default);
        assert!(!table.columns[1].is_generated);
        assert!(!table.columns[2].has_default);
    }

    #[test]
    fn flags_generated_and_identity_columns() {
        let table = one_table(
            "CREATE TABLE events (id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY, total INT GENERATED ALWAYS AS (1) STORED);",
        );
        assert!(table.columns[0].is_generated);
        assert!(table.columns[0].has_default);
        assert!(table.columns[1].is_generated);
        assert!(table.columns[1].has_default);
    }

    #[test]
    fn maps_time_and_arrays() {
        let table = one_table(
            "CREATE TABLE shifts (starts_at TIME NOT NULL, ends_at TIME, role_keys TEXT[] NOT NULL, weekday INTEGER[], branch_ids UUID[]);",
        );
        assert_eq!(table.columns[0].rust_type, "chrono::NaiveTime");
        assert_eq!(table.columns[1].rust_type, "Option<chrono::NaiveTime>");
        assert_eq!(table.columns[2].rust_type, "Vec<String>");
        assert_eq!(table.columns[3].rust_type, "Option<Vec<i32>>");
        assert_eq!(table.columns[4].rust_type, "Option<Vec<uuid::Uuid>>");
    }

    #[test]
    fn rejects_time_with_time_zone() {
        let parsed = parse("CREATE TABLE events (at TIME WITH TIME ZONE NOT NULL);");
        assert_eq!(parsed.schema.tables.len(), 0);
        assert!(parsed.failures[0].reason.contains("not supported"));
    }

    #[test]
    fn rejects_arrays_of_unsupported_elements() {
        let parsed = parse("CREATE TABLE shifts (spans INTERVAL[] NOT NULL);");
        assert_eq!(parsed.schema.tables.len(), 0);
        assert!(parsed.failures[0].reason.contains("not supported"));
    }

    #[test]
    fn primary_key_implies_not_null() {
        let table = one_table("CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);");
        assert!(!table.columns[0].nullable);
        assert_eq!(table.columns[0].rust_type, "uuid::Uuid");
    }

    #[test]
    fn stores_single_column_foreign_keys() {
        let table = one_table(
            "CREATE TABLE posts (id BIGSERIAL PRIMARY KEY, author_id UUID REFERENCES users (id));",
        );
        assert_eq!(table.foreign_keys.len(), 1);
        assert_eq!(table.foreign_keys[0].column, "author_id");
        assert_eq!(table.foreign_keys[0].referenced_table, "users");
        assert_eq!(table.foreign_keys[0].referenced_column, "id");
    }

    #[test]
    fn stores_table_level_foreign_keys() {
        let table = one_table(
            "CREATE TABLE posts (id BIGSERIAL PRIMARY KEY, author_id UUID, FOREIGN KEY (author_id) REFERENCES users (id));",
        );
        assert_eq!(table.foreign_keys.len(), 1);
        assert_eq!(table.foreign_keys[0].referenced_table, "users");
    }

    #[test]
    fn renames_keyword_columns() {
        let table = one_table("CREATE TABLE articles (id BIGSERIAL PRIMARY KEY, \"type\" TEXT);");
        assert_eq!(table.columns[1].rust_name, "type_");
        assert_eq!(table.columns[1].sql_name, "type");
    }

    #[test]
    fn unqualified_tables_have_no_schema() {
        let table = one_table("CREATE TABLE users (id BIGSERIAL PRIMARY KEY);");
        assert_eq!(table.schema, None);
        assert_eq!(table.display_name(), "users");
    }

    #[test]
    fn keeps_quoted_schema_case() {
        let table = one_table("CREATE TABLE \"Sales\".orders (id BIGSERIAL PRIMARY KEY);");
        assert_eq!(table.schema.as_deref(), Some("Sales"));
    }

    #[test]
    fn maps_unusual_identifiers_to_valid_rust_names() {
        let table = one_table(
            "CREATE TABLE \"Reserved Words\" (\"select\" BIGSERIAL PRIMARY KEY, \"1st\" BOOLEAN, \"Mixed Case\" INT);",
        );
        assert_eq!(table.module_name, "reserved_words");
        assert_eq!(table.struct_name, "ReservedWord");
        assert_eq!(table.columns[0].rust_name, "select");
        assert_eq!(table.columns[1].rust_name, "_1st");
        assert_eq!(table.columns[1].sql_name, "1st");
        assert_eq!(table.columns[2].rust_name, "mixed_case");
    }

    #[test]
    fn rejects_columns_without_a_rust_identifier() {
        let parsed = parse("CREATE TABLE odd (id BIGSERIAL PRIMARY KEY, \"%\" TEXT);");
        assert_eq!(parsed.schema.tables.len(), 0);
        assert!(parsed.failures[0].reason.contains("column `%`"));
        assert!(parsed.failures[0].reason.contains("Rust identifier"));
    }

    #[test]
    fn detects_rust_field_collisions() {
        let parsed = parse("CREATE TABLE odd (id BIGSERIAL PRIMARY KEY, \"Foo\" TEXT, foo TEXT);");
        assert_eq!(parsed.schema.tables.len(), 0);
        assert!(parsed.failures[0].reason.contains("already taken"));
    }

    #[test]
    fn collects_failures_per_table() {
        let parsed =
            parse("CREATE TABLE good (id BIGSERIAL PRIMARY KEY); CREATE TABLE bad (x INTERVAL);");
        assert_eq!(parsed.schema.tables.len(), 1);
        assert_eq!(parsed.failures.len(), 1);
        assert_eq!(parsed.failures[0].table, "bad");
    }

    #[test]
    fn rejects_columns_whose_generated_search_names_collide() {
        let null_check =
            parse("CREATE TABLE t (id BIGSERIAL PRIMARY KEY, weight INT, weight_is_null BOOLEAN);");
        assert!(null_check.schema.tables.is_empty());
        assert!(
            null_check.failures[0]
                .reason
                .contains("null-check filter field `weight_is_null`"),
            "{:?}",
            null_check.failures
        );

        let sort_key = parse("CREATE TABLE t (id BIGSERIAL PRIMARY KEY, item_1 INT, item1 INT);");
        assert!(sort_key.schema.tables.is_empty());
        assert!(
            sort_key.failures[0]
                .reason
                .contains("sort key variant `Item1`"),
            "{:?}",
            sort_key.failures
        );
    }

    #[test]
    fn rejects_tables_named_after_generated_modules() {
        let parsed = parse(
            "CREATE TABLE search (id BIGSERIAL PRIMARY KEY); CREATE TABLE schema (id BIGSERIAL PRIMARY KEY);",
        );
        assert!(parsed.schema.tables.is_empty());
        assert_eq!(parsed.failures.len(), 2);
        assert!(
            parsed.failures[0]
                .reason
                .contains("reserved for the generated `db/search.rs`")
        );
    }

    #[test]
    fn rejects_module_name_collisions() {
        let parsed = parse(
            "CREATE TABLE users (id BIGSERIAL PRIMARY KEY); CREATE TABLE \"Users\" (id BIGSERIAL PRIMARY KEY);",
        );
        assert_eq!(parsed.schema.tables.len(), 1);
        assert_eq!(parsed.failures.len(), 1);
    }

    #[test]
    fn ignores_non_create_table_statements() {
        let table = one_table(
            "CREATE INDEX idx ON users (id); CREATE TABLE users (id BIGSERIAL PRIMARY KEY);",
        );
        assert_eq!(table.sql_name, "users");
    }

    #[test]
    fn reports_empty_schema() {
        assert!(parse_schema("SELECT 1;").is_err());
    }
}
