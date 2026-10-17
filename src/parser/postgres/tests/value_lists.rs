use super::*;

#[test]
fn value_list_columns_become_enums_named_after_table_and_column() {
    let parsed = parse(
        "CREATE TABLE catalog.products (
                 id BIGSERIAL PRIMARY KEY,
                 product_type TEXT NOT NULL DEFAULT 'stocked'
                     CHECK (product_type IN ('stocked', 'Made To Order', 'stocked')),
                 measures VARCHAR(20),
                 type TEXT CHECK (type IN ('a', 'b')),
                 CONSTRAINT measures_known CHECK (measures IN ('count', 'weight'))
             );
             CREATE TABLE orders (
                 id BIGSERIAL PRIMARY KEY,
                 status TEXT NOT NULL CHECK (status IN ('open', 'closed'))
             );
             CREATE TABLE invoices (
                 id BIGSERIAL PRIMARY KEY,
                 status TEXT NOT NULL CHECK (status IN ('draft', 'paid'))
             );",
    );
    assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
    assert_eq!(
        enum_summary(&parsed.schema),
        [
            (
                "ProductProductType".to_owned(),
                Some("catalog.products.product_type"),
                vec!["stocked", "Made To Order"],
            ),
            (
                "ProductMeasures".to_owned(),
                Some("catalog.products.measures"),
                vec!["count", "weight"],
            ),
            (
                "ProductType".to_owned(),
                Some("catalog.products.type"),
                vec!["a", "b"],
            ),
            (
                "OrderStatus".to_owned(),
                Some("orders.status"),
                vec!["open", "closed"],
            ),
            (
                "InvoiceStatus".to_owned(),
                Some("invoices.status"),
                vec!["draft", "paid"],
            ),
        ]
    );
    let products = &parsed.schema.tables[0].columns;
    assert_eq!(products[1].rust_type, "types::ProductProductType");
    assert_eq!(products[1].sqlx_type, "types::ProductProductType");
    assert!(products[1].has_default);
    assert_eq!(products[2].rust_type, "Option<types::ProductMeasures>");
    assert_eq!(
        products[2].filter_kind(),
        Some(crate::schema::FilterKind::Equality)
    );
    assert_eq!(products[3].rust_type, "Option<types::ProductType>");
    assert_eq!(
        parsed.schema.tables[2].columns[1].rust_type,
        "types::InvoiceStatus"
    );
}

#[test]
fn checks_of_any_other_shape_leave_columns_as_they_are() {
    let table = one_table(
        "CREATE TABLE things (
                 id BIGSERIAL PRIMARY KEY,
                 negated TEXT CHECK (negated NOT IN ('a')),
                 numbers INTEGER CHECK (numbers IN (1, 2)),
                 mixed TEXT CHECK (mixed IN ('a', 1)),
                 combined TEXT CHECK (combined IN ('a') OR combined IS NULL),
                 pattern TEXT CHECK (pattern ~ '^[a-z]+$'),
                 tags TEXT[] CHECK (tags IN ('a')),
                 CHECK (length(pattern) > 1)
             );",
    );
    let types: Vec<&str> = table
        .columns
        .iter()
        .map(|column| column.rust_type.as_str())
        .collect();
    assert_eq!(
        types,
        [
            "i64",
            "Option<String>",
            "Option<i32>",
            "Option<String>",
            "Option<String>",
            "Option<String>",
            "Option<Vec<String>>",
        ]
    );
}

#[test]
fn value_lists_follow_alter_table() {
    let parsed = parse_files(&[
        (
            "0001.up.sql",
            "CREATE TABLE orders (id BIGSERIAL PRIMARY KEY, state TEXT NOT NULL);
                 ALTER TABLE orders ADD CONSTRAINT state_known CHECK (state IN ('new', 'done'));
                 ALTER TABLE orders ADD COLUMN channel TEXT CHECK (channel IN ('web', 'shop'));",
        ),
        (
            "0002.up.sql",
            "ALTER TABLE orders RENAME COLUMN state TO status;",
        ),
    ]);
    assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
    assert_eq!(
        enum_summary(&parsed.schema),
        [
            (
                "OrderStatus".to_owned(),
                Some("orders.status"),
                vec!["new", "done"],
            ),
            (
                "OrderChannel".to_owned(),
                Some("orders.channel"),
                vec!["web", "shop"],
            ),
        ]
    );
}

#[test]
fn value_lists_that_cannot_be_generated_fail_their_tables() {
    let parsed = parse(
            "CREATE TYPE order_status AS ENUM ('a');
             CREATE TABLE clash (id BIGSERIAL PRIMARY KEY, x TEXT CHECK (x IN ('in progress', 'IN_PROGRESS')));
             CREATE TABLE blank (id BIGSERIAL PRIMARY KEY, x TEXT CHECK (x IN ('!!')));
             CREATE TABLE orders (id BIGSERIAL PRIMARY KEY, status TEXT CHECK (status IN ('a')));
             CREATE TABLE fine (id BIGSERIAL PRIMARY KEY, x TEXT CHECK (x IN ('a')));",
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
                "clash",
                "column `x`: value list cannot be generated: labels `in progress` and `IN_PROGRESS` both map to the variant `InProgress`"
            ),
            (
                "blank",
                "column `x`: value list cannot be generated: label `!!` has no characters usable in a Rust identifier"
            ),
            (
                "orders",
                "column `status`: value list cannot be generated: its Rust name `OrderStatus` is already used by another enum type"
            ),
        ]
    );
    assert_eq!(parsed.failures[0].line, Some(2));
    let names: Vec<String> = enum_summary(&parsed.schema)
        .into_iter()
        .map(|(name, ..)| name)
        .collect();
    assert_eq!(names, ["OrderStatus", "FineX"]);
}
