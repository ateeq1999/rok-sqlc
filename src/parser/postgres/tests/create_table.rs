use super::*;

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
    let parsed = parse("CREATE TABLE shifts (id BIGSERIAL PRIMARY KEY, span POINT NOT NULL);");
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
    let table = one_table("CREATE TABLE \"Users\" (ID UUID PRIMARY KEY, \"Email\" TEXT NOT NULL);");
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
fn maps_interval_bytea_and_time_with_time_zone() {
    let table = one_table(
        "CREATE TABLE shifts (span INTERVAL NOT NULL, spans INTERVAL[], badge BYTEA NOT NULL, \
             photos BYTEA[], opens TIME WITH TIME ZONE NOT NULL, closes TIMETZ, slots TIMETZ[]);",
    );
    let types: Vec<&str> = table
        .columns
        .iter()
        .map(|column| column.rust_type.as_str())
        .collect();
    assert_eq!(
        types,
        [
            "sqlx::postgres::types::PgInterval",
            "Option<Vec<sqlx::postgres::types::PgInterval>>",
            "Vec<u8>",
            "Option<Vec<Vec<u8>>>",
            "sqlx::postgres::types::PgTimeTz",
            "Option<sqlx::postgres::types::PgTimeTz>",
            "Option<Vec<sqlx::postgres::types::PgTimeTz>>",
        ]
    );
    // SQLx implements no `PartialOrd` for `PgInterval` or `PgTimeTz`, so
    // they get equality filters, as `BYTEA` does; arrays get none.
    let equality = Some(crate::schema::FilterKind::Equality);
    let kinds: Vec<_> = table
        .columns
        .iter()
        .map(|column| column.filter_kind())
        .collect();
    assert_eq!(
        kinds,
        [equality, None, equality, None, equality, equality, None]
    );
}

#[test]
fn rejects_arrays_of_unsupported_elements() {
    let parsed = parse("CREATE TABLE hosts (addresses POINT[] NOT NULL);");
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
    assert_eq!(table.foreign_keys[0].referenced_schema, None);
    assert_eq!(table.foreign_keys[0].referenced_table, "users");
}

#[test]
fn foreign_keys_keep_the_referenced_schema() {
    let table = one_table(
        "CREATE TABLE posts (
                id BIGSERIAL PRIMARY KEY,
                author_id UUID REFERENCES core.users (id),
                editor_id UUID,
                FOREIGN KEY (editor_id) REFERENCES \"Audit\".users (id)
            );",
    );
    let schemas: Vec<Option<&str>> = table
        .foreign_keys
        .iter()
        .map(|key| key.referenced_schema.as_deref())
        .collect();
    assert_eq!(schemas, [Some("core"), Some("Audit")]);
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
    let parsed = parse("CREATE TABLE good (id BIGSERIAL PRIMARY KEY); CREATE TABLE bad (x POINT);");
    assert_eq!(parsed.schema.tables.len(), 1);
    assert_eq!(parsed.failures.len(), 1);
    assert_eq!(parsed.failures[0].table, "bad");
}

#[test]
fn rejects_tables_named_after_generated_modules() {
    let parsed = parse(
        "CREATE TABLE search (id BIGSERIAL PRIMARY KEY); CREATE TABLE schema (id BIGSERIAL PRIMARY KEY); CREATE TABLE session (id BIGSERIAL PRIMARY KEY); CREATE TABLE nullable (id BIGSERIAL PRIMARY KEY);",
    );
    assert!(parsed.schema.tables.is_empty());
    assert_eq!(parsed.failures.len(), 4);
    assert!(
        parsed.failures[3]
            .reason
            .contains("reserved for the generated `db/nullable.rs`")
    );
    assert!(
        parsed.failures[0]
            .reason
            .contains("reserved for the generated `db/search.rs`")
    );
    assert!(
        parsed.failures[2]
            .reason
            .contains("reserved for the generated `db/session.rs`")
    );
}

/// `db/http/` is generated only with `extractors`, so the parser leaves the
/// name free; a run with extractors rejects it (`generator::table_failures`).
#[test]
fn keeps_a_table_named_http() {
    let parsed = parse("CREATE TABLE http (id BIGSERIAL PRIMARY KEY);");
    assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
    assert_eq!(parsed.schema.tables.len(), 1);
    assert_eq!(parsed.schema.tables[0].module_name, "http");
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
    let table =
        one_table("CREATE INDEX idx ON users (id); CREATE TABLE users (id BIGSERIAL PRIMARY KEY);");
    assert_eq!(table.sql_name, "users");
}

#[test]
fn reports_empty_schema() {
    assert!(parse_schema("SELECT 1;").is_err());
}
