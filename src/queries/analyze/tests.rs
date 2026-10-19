use super::*;
use crate::queries::annotations::parse_query_file;

const SCHEMA: &str = "
        CREATE TABLE categories (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL);
        CREATE TABLE products (
            id UUID PRIMARY KEY,
            sku TEXT NOT NULL,
            name TEXT NOT NULL,
            category_id BIGINT NOT NULL REFERENCES categories (id),
            note TEXT,
            stock INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE app.tags (id BIGSERIAL PRIMARY KEY, \"select\" TEXT NOT NULL);
        CREATE TABLE spans (id BIGSERIAL PRIMARY KEY, length POINT NOT NULL);
        CREATE TYPE product_status AS ENUM ('active', 'retired');
        CREATE TYPE app.ticket_state AS ENUM ('open', 'closed');
        CREATE TABLE gauges (id BIGSERIAL PRIMARY KEY, level TEXT CHECK (level IN ('low', 'high')));
        CREATE TABLE nodes (id BIGSERIAL PRIMARY KEY, parent_id BIGINT REFERENCES nodes (id), label TEXT NOT NULL);
        CREATE TABLE members (id BIGSERIAL PRIMARY KEY, email TEXT NOT NULL, deleted_at TIMESTAMPTZ);
        CREATE UNIQUE INDEX members_email_live ON members (email) WHERE deleted_at IS NULL;
        CREATE TABLE handles (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL);
        CREATE UNIQUE INDEX handles_name_ci ON handles (lower(name));
    ";

fn analyze(queries: &str) -> (Vec<AnalyzedQuery>, Vec<QueryFailure>) {
    let parsed = crate::parser::parse_schema(SCHEMA).unwrap();
    let (defs, failures) = parse_query_file(queries);
    assert!(failures.is_empty(), "{failures:?}");
    let modules = parsed
        .schema
        .tables
        .iter()
        .map(|table| table.module_name.clone())
        .collect();
    analyze_queries(
        &defs,
        &parsed.schema.tables,
        &parsed.schema.enums,
        &modules,
        &parsed.failures,
    )
}

fn one(queries: &str) -> AnalyzedQuery {
    let (mut analyzed, failures) = analyze(queries);
    assert!(failures.is_empty(), "{failures:?}");
    analyzed.remove(0)
}

fn failure(queries: &str) -> String {
    let (analyzed, failures) = analyze(queries);
    assert!(analyzed.is_empty(), "expected a failure, got {analyzed:?}");
    failures[0].reason.clone()
}

fn param(index: usize, name: &str, rust_type: &str) -> Param {
    Param {
        index,
        name: name.to_owned(),
        rust_type: rust_type.to_owned(),
    }
}

#[test]
fn infers_params_from_comparisons_and_limits() {
    let query = one(
        "-- name: ListProducts :many\nSELECT id, sku FROM products WHERE category_id = $1 AND stock <= $2 ORDER BY sku LIMIT $3 OFFSET $4;",
    );
    assert_eq!(query.fn_name, "list_products");
    assert_eq!(
        query.params,
        [
            param(1, "category_id", "i64"),
            param(2, "stock", "i32"),
            param(3, "limit", "i64"),
            param(4, "offset", "i64"),
        ]
    );
    assert_eq!(
        query.sql,
        "SELECT id, sku FROM products WHERE category_id = $1 AND stock <= $2 ORDER BY sku LIMIT $3 OFFSET $4"
    );
    let ResultShape::Row(columns) = &query.result else {
        panic!("expected a row struct, got {:?}", query.result);
    };
    assert_eq!(columns[0].rust_type, "uuid::Uuid");
    assert_eq!(columns[1].rust_name, "sku");
}

#[test]
fn casts_to_interval_bytea_and_timetz_infer_their_mapped_types() {
    let query = one(
        "-- name: Q :many\nSELECT id, '1 day'::interval AS span, sku::bytea AS raw, '12:00+01'::timetz AS opens \
             FROM products WHERE $1::interval IS NOT NULL AND $2::bytea IS NOT NULL AND $3::time with time zone IS NOT NULL;",
    );
    let interval = "sqlx::postgres::types::PgInterval";
    let time_tz = "sqlx::postgres::types::PgTimeTz";
    assert_eq!(
        query.params,
        [
            param(1, "param_1", interval),
            param(2, "param_2", "Vec<u8>"),
            param(3, "param_3", time_tz),
        ]
    );
    let ResultShape::Row(columns) = &query.result else {
        panic!("expected a row struct");
    };
    let types: Vec<&str> = columns
        .iter()
        .map(|column| column.rust_type.as_str())
        .collect();
    assert_eq!(
        types[1..],
        [
            format!("Option<{interval}>"),
            "Option<Vec<u8>>".to_owned(),
            format!("Option<{time_tz}>"),
        ]
    );
}

#[test]
fn infers_params_from_in_any_between_like_and_casts() {
    let query = one(
        "-- name: Search :many\nSELECT id, sku FROM products WHERE sku LIKE $1 AND stock BETWEEN $2 AND $3 AND category_id IN ($4, $5) AND id = ANY($6) AND name = $7::text;",
    );
    assert_eq!(
        query.params,
        [
            param(1, "sku", "String"),
            param(2, "stock", "i32"),
            param(3, "stock_2", "i32"),
            param(4, "category_id", "i64"),
            param(5, "category_id_2", "i64"),
            param(6, "id_list", "Vec<uuid::Uuid>"),
            param(7, "name", "String"),
        ]
    );
}

#[test]
fn insert_values_take_column_types_and_nullability() {
    let query = one(
        "-- name: CreateProducts :execrows\nINSERT INTO products (id, sku, name, category_id, note) VALUES ($1, $2, $3, $4, $5), ($6, $7, $8, $9, NULL);",
    );
    assert_eq!(query.params[4], param(5, "note", "Option<String>"));
    assert_eq!(query.params[5], param(6, "id_2", "uuid::Uuid"));
    assert_eq!(query.params.len(), 9);
    assert_eq!(query.result, ResultShape::None);
}

#[test]
fn update_set_and_upsert_assignments_are_typed() {
    let query =
        one("-- name: Rename :exec\nUPDATE products SET name = $2, note = $3 WHERE id = $1;");
    assert_eq!(
        query.params,
        [
            param(1, "id", "uuid::Uuid"),
            param(2, "name", "String"),
            param(3, "note", "Option<String>"),
        ]
    );
    let upsert = one(
        "-- name: Upsert :exec\nINSERT INTO products (id, sku, name, category_id) VALUES ($1, $2, $3, $4)\nON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, stock = $5;",
    );
    assert_eq!(upsert.params[4], param(5, "stock", "i32"));
}

#[test]
fn returning_star_reuses_the_resource_model() {
    let query =
        one("-- name: CreateCategory :one\nINSERT INTO categories (name) VALUES ($1) RETURNING *;");
    assert_eq!(
        query.result,
        ResultShape::Model {
            module: "categories".to_owned(),
            struct_name: "Category".to_owned()
        }
    );
}

#[test]
fn casts_to_enum_types_infer_the_generated_enum() {
    let query = one(
        "-- name: Statuses :many\nSELECT id FROM products WHERE $1::product_status IS NOT NULL AND $2::app.ticket_state IS NOT NULL AND $3::product_status[] IS NOT NULL AND $4::public.product_status IS NOT NULL;",
    );
    assert_eq!(
        query.params,
        [
            param(1, "param_1", "types::ProductStatus"),
            param(2, "param_2", "types::TicketState"),
            param(3, "param_3", "Vec<types::ProductStatus>"),
            param(4, "param_4", "types::ProductStatus"),
        ]
    );
    let status =
        one("-- name: Status :one\nSELECT 'active'::product_status AS status FROM products;");
    assert_eq!(
        status.result,
        ResultShape::Scalar("Option<types::ProductStatus>".to_owned())
    );
}

/// An enum type is found under its schema, and a value-list enum is no
/// database type at all.
#[test]
fn casts_to_other_names_are_not_enums() {
    for cast in ["ticket_state", "app.product_status", "level"] {
        let reason = failure(&format!(
            "-- name: Q :many\nSELECT id FROM gauges WHERE $1::{cast} IS NOT NULL;"
        ));
        assert!(reason.contains("is not supported yet"), "{cast}: {reason}");
    }
}

#[test]
fn partial_selects_get_row_structs_and_single_columns_scalars() {
    let row = one("-- name: Names :many\nSELECT name, id FROM categories;");
    assert!(matches!(row.result, ResultShape::Row(_)));
    let scalar = one("-- name: CountProducts :one\nSELECT COUNT(*) FROM products;");
    assert_eq!(scalar.result, ResultShape::Scalar("i64".to_owned()));
    let note = one("-- name: Note :maybe_one\nSELECT note FROM products WHERE id = $1;");
    assert_eq!(
        note.result,
        ResultShape::Scalar("Option<String>".to_owned())
    );
}

#[test]
fn left_joins_make_the_right_side_nullable() {
    let query = one(
        "-- name: WithCategory :many\nSELECT p.id, p.sku, c.name AS category_name FROM products p LEFT JOIN categories c ON c.id = p.category_id WHERE c.name = $1;",
    );
    let ResultShape::Row(columns) = &query.result else {
        panic!("expected a row struct");
    };
    assert_eq!(columns[2].sql_name, "category_name");
    assert_eq!(columns[2].rust_type, "Option<String>");
    assert_eq!(query.params, [param(1, "name", "String")]);
}

#[test]
fn schema_qualified_and_quoted_names_resolve() {
    let query =
        one("-- name: Tags :many\nSELECT id, \"select\" FROM app.tags WHERE \"select\" = $1;");
    assert_eq!(
        query.result,
        ResultShape::Model {
            module: "tags".to_owned(),
            struct_name: "Tag".to_owned()
        }
    );
    assert_eq!(query.params, [param(1, "select", "String")]);
}

#[test]
fn overrides_supply_types_inference_cannot() {
    let query = one(
        "-- name: Totals :many\n-- param: $1 min_total i64\n-- column: total i64\nSELECT category_id, SUM(stock)::bigint AS total FROM products GROUP BY category_id HAVING SUM(stock) > $1;",
    );
    assert_eq!(query.params, [param(1, "min_total", "i64")]);
    let ResultShape::Row(columns) = &query.result else {
        panic!("expected a row struct");
    };
    assert_eq!(columns[1].rust_type, "i64");
}

#[test]
fn reports_untypable_and_unused_placeholders() {
    assert!(
        failure("-- name: Q :many\nSELECT id FROM products WHERE lower(name) = lower($1);")
            .contains("add `-- param: $1 <name> <RustType>`")
    );
    assert!(
        failure("-- name: Q :many\nSELECT id FROM products WHERE sku = $2;")
            .contains("`$1` is never used")
    );
    assert!(
        failure("-- name: Q :many\nSELECT id FROM products WHERE sku = $1 OR stock = $1;")
            .contains("used both as `String` and as `i32`")
    );
}

#[test]
fn reports_schema_mismatches() {
    assert!(
        failure("-- name: Q :many\nSELECT id FROM nope;").contains("`nope` is not in the schema")
    );
    assert!(
        failure("-- name: Q :many\nSELECT colour FROM products;")
            .contains("column `colour` does not exist in `products`")
    );
    assert!(
            failure(
                "-- name: Q :many\nSELECT id, name FROM products p JOIN categories c ON c.id = p.category_id;"
            )
            .contains("ambiguous")
        );
    assert!(
        failure("-- name: Q :many\nSELECT id FROM spans;")
            .contains("table `spans` is not generatable")
    );
}

#[test]
fn reports_unsupported_statement_shapes() {
    assert!(
        failure("-- name: Q :exec\nBEGIN;\nDELETE FROM products;\nCOMMIT;")
            .contains("contains 3 statements")
    );
    assert!(
        failure("-- name: Q :one\nDELETE FROM products WHERE id = $1;")
            .contains("add RETURNING or use `:exec`")
    );
    assert!(
        failure("-- name: Q :many\nSELECT upper(name) FROM products;")
            .contains("`-- column: upper <RustType>`")
    );
    assert!(
        failure("-- name: Q :many\nSELECT id, id FROM products;")
            .contains("appears more than once")
    );
    assert!(failure("-- name: Q :many\nSELECT 1 FROM products;").contains("needs an alias"));
}

fn row_columns(query: &AnalyzedQuery) -> Vec<(&str, &str)> {
    let ResultShape::Row(columns) = &query.result else {
        panic!("expected a row struct, got {:?}", query.result);
    };
    columns
        .iter()
        .map(|column| (column.sql_name.as_str(), column.rust_type.as_str()))
        .collect()
}

#[test]
fn cte_results_join_like_tables_and_type_their_placeholders() {
    let query = one("-- name: StockByCategory :many\n\
             WITH stock AS (\n\
                 SELECT category_id, SUM(stock)::bigint AS total FROM products WHERE stock >= $1 GROUP BY category_id\n\
             )\n\
             SELECT c.id, c.name, s.total FROM categories c JOIN stock s ON s.category_id = c.id WHERE s.total > $2;");
    assert_eq!(
        row_columns(&query),
        [("id", "i64"), ("name", "String"), ("total", "Option<i64>")]
    );
    assert_eq!(
        query.params,
        [param(1, "stock", "i32"), param(2, "total", "i64")]
    );
}

#[test]
fn later_ctes_see_earlier_ones_and_column_lists_rename() {
    let query = one("-- name: BigCategories :many\n\
             WITH counted (cat, n) AS (SELECT category_id, COUNT(*) FROM products GROUP BY category_id),\n\
                  big AS (SELECT cat, n AS product_count FROM counted WHERE n > $1)\n\
             SELECT big.cat, big.product_count, c.name FROM big JOIN categories c ON c.id = big.cat;");
    assert_eq!(
        row_columns(&query),
        [("cat", "i64"), ("product_count", "i64"), ("name", "String")]
    );
    assert_eq!(query.params, [param(1, "n", "i64")]);
}

#[test]
fn a_cte_column_list_names_unaliased_expressions() {
    let query = one(
        "-- name: Totals :many\nWITH t (category_id, total) AS (SELECT category_id, SUM(stock)::bigint FROM products GROUP BY category_id) SELECT category_id, total FROM t;",
    );
    assert_eq!(
        row_columns(&query),
        [("category_id", "i64"), ("total", "Option<i64>")]
    );
    assert!(
        failure(
            "-- name: Q :many\nWITH t AS (SELECT SUM(stock)::bigint FROM products) SELECT * FROM t;"
        )
        .contains("needs an alias")
    );
}

#[test]
fn a_cte_selecting_a_whole_table_does_not_reuse_its_model() {
    let query =
        one("-- name: Cats :many\nWITH cats AS (SELECT * FROM categories) SELECT * FROM cats;");
    assert_eq!(row_columns(&query), [("id", "i64"), ("name", "String")]);
}

#[test]
fn ctes_are_scoped_to_their_statement_and_must_be_typable() {
    assert!(
            failure("-- name: Q :many\nWITH a AS (SELECT id FROM b), b AS (SELECT id FROM categories) SELECT id FROM a;")
                .contains("table `b` is not in the schema")
        );
    assert!(
            failure("-- name: Q :many\nWITH a AS (SELECT SUM(stock) AS total FROM products) SELECT total FROM a;")
                .contains("cannot infer the type of column `total` of CTE `a`")
        );
    assert!(
        failure("-- name: Q :many\nWITH a (x, y) AS (SELECT id FROM categories) SELECT x FROM a;")
            .contains("CTE `a` names 2 columns but its query returns 1")
    );
}

/// The anchor types a recursive CTE; the recursive term sees the CTE and
/// can make a column nullable.
#[test]
fn recursive_ctes_take_their_columns_from_the_anchor() {
    let walk = one(
        "-- name: Subtree :many\nWITH RECURSIVE tree (id, parent_id, depth) AS (\n    SELECT id, parent_id, 0::int FROM nodes WHERE id = $1\n    UNION ALL\n    SELECT n.id, n.parent_id, t.depth + 1 FROM nodes n JOIN tree t ON n.parent_id = t.id\n)\nSELECT id, parent_id, depth FROM tree;",
    );
    assert_eq!(
        row_columns(&walk),
        [
            ("id", "i64"),
            ("parent_id", "Option<i64>"),
            // A cast may be NULL, as everywhere else.
            ("depth", "Option<i32>")
        ]
    );
    assert_eq!(walk.params, [param(1, "id", "i64")]);

    let widened = one(
        "-- name: Labels :many\nWITH RECURSIVE t AS (\n    SELECT id, label FROM nodes WHERE parent_id IS NULL\n    UNION\n    SELECT n.id, NULL::text FROM nodes n JOIN t ON n.parent_id = t.id\n)\nSELECT id, label FROM t;",
    );
    assert_eq!(
        row_columns(&widened),
        [("id", "i64"), ("label", "Option<String>")]
    );
}

#[test]
fn recursive_terms_must_match_the_anchor() {
    assert_eq!(
        failure(
            "-- name: Q :many\nWITH RECURSIVE t (n) AS (SELECT 1::int UNION ALL SELECT n, n FROM t) SELECT n FROM t;"
        ),
        "the recursive term of CTE `t` returns 2 columns but its anchor returns 1"
    );
    assert_eq!(
        failure(
            "-- name: Q :many\nWITH RECURSIVE t (n) AS (SELECT 1::int UNION ALL SELECT n::bigint FROM t) SELECT n FROM t;"
        ),
        "the recursive term of CTE `t` returns `i64` for column `n`, but its anchor returns `i32`; cast one of them"
    );
}

#[test]
fn data_modifying_ctes_are_rejected() {
    assert!(
            failure(
                "-- name: Q :many\nWITH gone AS (DELETE FROM products WHERE stock = $1 RETURNING id) SELECT id FROM gone;"
            )
            .contains("data-modifying CTE `gone`")
        );
}

/// A partial unique index is a target when the target repeats its
/// condition; the generated function keeps the SQL as written.
#[test]
fn partial_unique_index_targets_need_their_condition() {
    let upsert = one(
        "-- name: UpsertMember :one\nINSERT INTO members (email) VALUES ($1)\nON CONFLICT (email) WHERE (deleted_at IS NULL) DO UPDATE SET email = EXCLUDED.email\nRETURNING id;",
    );
    assert!(
        upsert.sql.contains("WHERE (deleted_at IS NULL) DO UPDATE"),
        "{}",
        upsert.sql
    );
    assert_eq!(upsert.params, [param(1, "email", "String")]);

    // A full key matches with any predicate, as PostgreSQL infers it too.
    one(
        "-- name: ById :exec\nINSERT INTO members (id, email) VALUES ($1, $2)\nON CONFLICT (id) WHERE deleted_at IS NULL DO NOTHING;",
    );

    assert_eq!(
        failure(
            "-- name: Missing :exec\nINSERT INTO members (email) VALUES ($1) ON CONFLICT (email) DO NOTHING;"
        ),
        "ON CONFLICT (email) matches only a partial unique index of `members`, whose condition is `deleted_at IS NULL`; add `WHERE deleted_at IS NULL` to the target"
    );
    assert_eq!(
        failure(
            "-- name: Other :exec\nINSERT INTO members (email) VALUES ($1) ON CONFLICT (email) WHERE deleted_at IS NOT NULL DO NOTHING;"
        ),
        "ON CONFLICT (email) matches only a partial unique index of `members`, whose condition is `deleted_at IS NULL`; the target's `WHERE deleted_at IS NOT NULL` must be `WHERE deleted_at IS NULL`"
    );
    assert!(
        failure("-- name: Bound :exec\nINSERT INTO members (email) VALUES ($1) ON CONFLICT (email) WHERE email <> $2 DO NOTHING;")
            .starts_with("a placeholder in the ON CONFLICT target's WHERE is not supported"),
    );
}

/// Taking the predicate out keeps the rest of the text as written, so
/// escaped quotes and other lines survive analysis.
#[test]
fn partial_index_targets_keep_escaped_quotes() {
    one(
        "-- name: Quoted :exec\nINSERT INTO members (email) VALUES ('it''s')\nON CONFLICT (email)\n    WHERE deleted_at IS NULL\nDO UPDATE SET email = 'O''Brien';",
    );
}

/// PostgreSQL infers a unique index on `lower(name)` only from the target
/// `(lower(name))`, so the column target `(name)` matches no unique key.
#[test]
fn case_insensitive_unique_keys_are_not_column_targets() {
    assert_eq!(
        failure(
            "-- name: Upsert :exec\nINSERT INTO handles (name) VALUES ($1) ON CONFLICT (name) DO NOTHING;"
        ),
        "ON CONFLICT (name) matches no unique key; unique keys of `handles`: (id)"
    );
}
