use super::*;

fn variant_names(schema: &Schema) -> Vec<Vec<(&str, &str)>> {
    schema
        .enums
        .iter()
        .map(|enum_type| {
            enum_type
                .variants
                .iter()
                .map(|variant| (variant.label.as_str(), variant.rust_name.as_str()))
                .collect()
        })
        .collect()
}

#[test]
fn enum_columns_use_the_shared_types_module() {
    let parsed = parse(
        "CREATE TYPE mood AS ENUM ('Happy', 'SAD', 'so-so', 'Very High', 'self', '9 lives');
             CREATE TYPE app.\"Ticket State\" AS ENUM ('open');
             CREATE TABLE people (
                 id BIGSERIAL PRIMARY KEY,
                 mood MOOD NOT NULL,
                 state app.\"Ticket State\",
                 history public.mood[]
             );",
    );
    assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
    let enums = &parsed.schema.enums;
    assert_eq!(enums[0].schema, None);
    assert_eq!(enums[0].sql_name, "mood");
    assert_eq!(enums[0].rust_name, "Mood");
    assert_eq!(enums[1].schema.as_deref(), Some("app"));
    assert_eq!(enums[1].sql_name, "Ticket State");
    assert_eq!(enums[1].rust_name, "TicketState");
    assert_eq!(
        variant_names(&parsed.schema)[0],
        [
            ("Happy", "Happy"),
            ("SAD", "Sad"),
            ("so-so", "SoSo"),
            ("Very High", "VeryHigh"),
            ("self", "Self_"),
            ("9 lives", "V9Lives"),
        ]
    );

    let columns = &parsed.schema.tables[0].columns;
    assert_eq!(columns[1].rust_type, "types::Mood");
    assert_eq!(
        columns[1].filter_kind(),
        Some(crate::schema::FilterKind::Equality)
    );
    assert_eq!(columns[2].rust_type, "Option<types::TicketState>");
    assert_eq!(columns[2].sqlx_type, "types::TicketState");
    assert_eq!(columns[3].rust_type, "Option<Vec<types::Mood>>");
    assert_eq!(columns[3].filter_kind(), None);
}

#[test]
fn enums_in_different_schemas_keep_apart() {
    let parsed = parse(
        "CREATE TYPE status AS ENUM ('a');
             CREATE TYPE billing.status AS ENUM ('b');
             CREATE TABLE invoices (
                 id BIGSERIAL PRIMARY KEY,
                 local STATUS NOT NULL,
                 billed billing.status NOT NULL
             );",
    );
    assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
    let names: Vec<&str> = parsed
        .schema
        .enums
        .iter()
        .map(|enum_type| enum_type.rust_name.as_str())
        .collect();
    assert_eq!(names, ["Status", "BillingStatus"]);
    let columns = &parsed.schema.tables[0].columns;
    assert_eq!(columns[1].rust_type, "types::Status");
    assert_eq!(columns[2].rust_type, "types::BillingStatus");
}

#[test]
fn alter_type_adds_and_renames_values() {
    let parsed = parse(
        "CREATE TYPE app.state AS ENUM ('open', 'closed');
             CREATE TABLE tickets (id BIGSERIAL PRIMARY KEY, state app.state NOT NULL);
             ALTER TYPE app.state ADD VALUE 'Archived';
             ALTER TYPE app.state ADD VALUE 'draft' BEFORE 'open';
             ALTER TYPE app.state ADD VALUE IF NOT EXISTS 'Reopened' AFTER 'closed';
             ALTER TYPE app.state ADD VALUE IF NOT EXISTS 'open';
             ALTER TYPE app.state RENAME VALUE 'closed' TO 'CLOSED';
             ALTER TYPE unknown_type ADD VALUE 'x';",
    );
    assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
    assert_eq!(
        variant_names(&parsed.schema)[0],
        [
            ("draft", "Draft"),
            ("open", "Open"),
            ("CLOSED", "Closed"),
            ("Reopened", "Reopened"),
            ("Archived", "Archived"),
        ]
    );
}

#[test]
fn drop_type_removes_the_enum_and_cascades_to_its_columns() {
    let parsed = parse(
        "CREATE TYPE app.state AS ENUM ('open', 'closed');
             CREATE TYPE mood AS ENUM ('ok');
             CREATE TABLE tickets (id BIGSERIAL PRIMARY KEY, state app.state NOT NULL, title TEXT);
             CREATE TABLE notes (id BIGSERIAL PRIMARY KEY, mood mood);
             DROP TYPE IF EXISTS app.state, missing CASCADE;
             DROP TYPE mood;",
    );
    let names: Vec<&str> = parsed
        .schema
        .enums
        .iter()
        .map(|enum_type| enum_type.sql_name.as_str())
        .collect();
    assert!(names.is_empty(), "{names:?}");
    let tickets = &parsed.schema.tables[0];
    assert_eq!(column_names(tickets), ["id", "title"]);
    assert_eq!(parsed.failures.len(), 1, "{:?}", parsed.failures);
    assert_eq!(parsed.failures[0].table, "notes");
    assert_eq!(parsed.failures[0].line, Some(6));
    assert!(
        parsed.failures[0].reason.contains("DROP TYPE `mood`"),
        "{}",
        parsed.failures[0].reason
    );
}

#[test]
fn drop_table_forgets_a_table_that_failed_earlier() {
    let parsed = parse(
        "CREATE TABLE users (id BIGSERIAL PRIMARY KEY);
             CREATE TABLE app.spans (id BIGSERIAL PRIMARY KEY, length POINT);
             CREATE TABLE spans (id BIGSERIAL PRIMARY KEY, length POINT);
             DROP TABLE app.spans;",
    );
    assert_eq!(parsed.failures.len(), 1, "{:?}", parsed.failures);
    assert_eq!(parsed.failures[0].line, Some(3));
}

#[test]
fn enum_types_that_cannot_be_generated_fail_their_tables() {
    let parsed = parse(
        "CREATE TYPE clash AS ENUM ('in progress', 'IN_PROGRESS');
             CREATE TYPE blank AS ENUM ('!!');
             CREATE TYPE fine AS ENUM ('a', 'b');
             CREATE TABLE a (id BIGSERIAL PRIMARY KEY, x clash);
             CREATE TABLE b (id BIGSERIAL PRIMARY KEY, x blank[]);
             CREATE TABLE c (id BIGSERIAL PRIMARY KEY, x fine);
             CREATE TABLE d (id BIGSERIAL PRIMARY KEY);
             ALTER TYPE fine ADD VALUE 'a';
             ALTER TYPE fine RENAME TO finer;",
    );
    let reasons: Vec<(&str, &str)> = parsed
        .failures
        .iter()
        .map(|failure| (failure.table.as_str(), failure.reason.as_str()))
        .collect();
    assert_eq!(
        reasons,
        [
            (
                "a",
                "column `x`: type `clash` cannot be generated: labels `in progress` and `IN_PROGRESS` both map to the variant `InProgress`"
            ),
            (
                "b",
                "column `x`: type `blank` cannot be generated: label `!!` has no characters usable in a Rust identifier"
            ),
            (
                "c",
                "ALTER TYPE `fine`: ADD VALUE: label `a` already exists"
            ),
        ]
    );
    assert_eq!(parsed.failures[2].line, Some(8));
    let tables: Vec<&str> = parsed
        .schema
        .tables
        .iter()
        .map(|table| table.sql_name.as_str())
        .collect();
    assert_eq!(tables, ["d"]);
    assert!(
        parsed.schema.enums.is_empty(),
        "only generatable enums are generated"
    );
}

#[test]
fn alter_type_rename_renames_the_enum_and_its_columns_follow() {
    let parsed = parse(
            "CREATE TYPE mood AS ENUM ('a');
             CREATE TABLE people (id BIGSERIAL PRIMARY KEY, mood mood NOT NULL, past mood[], next mood);
             ALTER TYPE mood RENAME TO feeling;
             CREATE TABLE later (id BIGSERIAL PRIMARY KEY, feeling feeling NOT NULL);",
        );
    assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
    let feeling = &parsed.schema.enums[0];
    assert_eq!(
        (feeling.sql_name.as_str(), feeling.rust_name.as_str()),
        ("feeling", "Feeling")
    );
    let types: Vec<(&str, &str)> = parsed
        .schema
        .tables
        .iter()
        .flat_map(|table| &table.columns[1..])
        .map(|column| (column.sqlx_type.as_str(), column.rust_type.as_str()))
        .collect();
    assert_eq!(
        types,
        [
            ("types::Feeling", "types::Feeling"),
            ("Vec<types::Feeling>", "Option<Vec<types::Feeling>>"),
            ("types::Feeling", "Option<types::Feeling>"),
            ("types::Feeling", "types::Feeling"),
        ]
    );
}

#[test]
fn alter_type_rename_keeps_the_schema_and_avoids_taken_rust_names() {
    let parsed = parse(
        "CREATE TYPE status AS ENUM ('a');
             CREATE TYPE app.mood AS ENUM ('b');
             CREATE TABLE app.people (id BIGSERIAL PRIMARY KEY, mood app.mood);
             ALTER TYPE app.mood RENAME TO status;",
    );
    assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
    let renamed = &parsed.schema.enums[1];
    assert_eq!(renamed.display_name(), "app.status");
    assert_eq!(renamed.rust_name, "AppStatus");
    assert_eq!(
        parsed.schema.tables[0].columns[1].sqlx_type,
        "types::AppStatus"
    );
}

#[test]
fn alter_type_rename_to_a_taken_name_fails_tables_using_the_type() {
    let parsed = parse(
        "CREATE TYPE mood AS ENUM ('a');
             CREATE TYPE feeling AS ENUM ('b');
             CREATE TABLE people (id BIGSERIAL PRIMARY KEY, mood mood);
             ALTER TYPE mood RENAME TO feeling;",
    );
    assert!(parsed.schema.tables.is_empty());
    assert_eq!(
        parsed.failures[0].reason,
        "ALTER TYPE `mood`: RENAME TO: type `feeling` already exists"
    );
}

#[test]
fn rejects_tables_named_types() {
    let parsed = parse("CREATE TABLE types (id BIGSERIAL PRIMARY KEY);");
    assert!(
        parsed.failures[0]
            .reason
            .contains("reserved for the generated `db/types.rs`")
    );
}
