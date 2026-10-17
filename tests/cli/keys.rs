//! Unique keys, `ON CONFLICT ON CONSTRAINT`, and default constraint names.

use super::*;

const UNIQUE_KEYS_SCHEMA: &str = "CREATE TABLE products (\n\
     id UUID PRIMARY KEY,\n\
     sku TEXT NOT NULL UNIQUE,\n\
     name TEXT NOT NULL,\n\
     organization_id UUID NOT NULL,\n\
     code TEXT NOT NULL,\n\
     barcode TEXT,\n\
     UNIQUE (organization_id, code)\n\
     );\n\
     CREATE UNIQUE INDEX products_lower_name ON products (lower(name));\n\
     CREATE UNIQUE INDEX products_barcode_set ON products (barcode) WHERE barcode <> '';\n";

#[test]
fn on_conflict_target_must_match_a_unique_key() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    let queries = dir.path().join("queries.sql");
    fs::write(&schema, UNIQUE_KEYS_SCHEMA)?;
    fs::write(
        &queries,
        "-- name: UpsertBySku :exec\n\
         INSERT INTO products (id, sku, name, organization_id, code) VALUES ($1, $2, $3, $4, $5)\n\
         ON CONFLICT (sku) DO UPDATE SET name = EXCLUDED.name;\n\
         \n\
         -- name: UpsertByCode :exec\n\
         INSERT INTO products (id, sku, name, organization_id, code) VALUES ($1, $2, $3, $4, $5)\n\
         ON CONFLICT (code, organization_id) DO NOTHING;\n\
         \n\
         -- name: UpsertByName :exec\n\
         INSERT INTO products (id, sku, name, organization_id, code) VALUES ($1, $2, $3, $4, $5)\n\
         ON CONFLICT (name) DO UPDATE SET sku = EXCLUDED.sku;\n\
         \n\
         -- name: UpsertByBarcode :exec\n\
         INSERT INTO products (id, sku, name, organization_id, code, barcode) VALUES ($1, $2, $3, $4, $5, $6)\n\
         ON CONFLICT (barcode) DO NOTHING;\n\
         \n\
         -- name: UpsertByLiveBarcode :exec\n\
         INSERT INTO products (id, sku, name, organization_id, code, barcode) VALUES ($1, $2, $3, $4, $5, $6)\n\
         ON CONFLICT (barcode) WHERE barcode <> '' DO NOTHING;\n",
    )?;
    let out = output_dir(&dir);

    let output = run_binary(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--queries",
        queries.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let path = shown(&queries);
    let keys = "unique keys of `products`: (id), (sku), (organization_id, code)";
    assert!(
        stderr.contains(&format!(
            "{path}:9: query `UpsertByName`: ON CONFLICT (name) matches no unique key; {keys}"
        )),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!(
            "{path}:13: query `UpsertByBarcode`: ON CONFLICT (barcode) matches only a partial unique index of `products`, whose condition is `barcode <> ''`; add `WHERE barcode <> ''` to the target"
        )),
        "{stderr}"
    );
    assert!(!stderr.contains("UpsertBySku"), "{stderr}");
    assert!(!stderr.contains("UpsertByCode"), "{stderr}");
    assert!(!stderr.contains("UpsertByLiveBarcode"), "{stderr}");
    assert!(stderr.contains("2 unsupported queries"), "{stderr}");
    Ok(())
}

/// `ON CONFLICT ON CONSTRAINT` must name a primary key or unique constraint:
/// declared names, or the ones PostgreSQL gives unnamed constraints
/// (`<table>_pkey`, `<table>_<columns>_key`). A unique index is no
/// constraint, so PostgreSQL rejects its name there.
#[test]
fn on_conflict_on_constraint_must_name_a_key_constraint() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    let queries = dir.path().join("queries.sql");
    fs::write(
        &schema,
        "CREATE TABLE app.products (\n\
         id UUID PRIMARY KEY,\n\
         sku TEXT NOT NULL UNIQUE,\n\
         name TEXT NOT NULL,\n\
         code TEXT NOT NULL,\n\
         barcode TEXT,\n\
         CONSTRAINT uq_code UNIQUE (code)\n\
         );\n\
         ALTER TABLE app.products ADD UNIQUE (name, code);\n\
         CREATE UNIQUE INDEX products_barcode_idx ON app.products (barcode);\n",
    )?;
    let upsert = |name: &str, target: &str| {
        format!(
            "-- name: {name} :exec\n\
             INSERT INTO app.products (id, sku, name, code) VALUES ($1, $2, $3, $4)\n\
             ON CONFLICT ON CONSTRAINT {target} DO NOTHING;\n\n"
        )
    };
    fs::write(
        &queries,
        [
            upsert("ByKey", "products_pkey"),
            upsert("BySku", "products_sku_key"),
            upsert("ByCode", "uq_code"),
            upsert("ByNameAndCode", "products_name_code_key"),
            upsert("ByBarcode", "products_barcode_idx"),
        ]
        .concat(),
    )?;
    let out = output_dir(&dir);

    let output = run_binary(&[
        "g",
        "-s",
        schema.to_str().unwrap(),
        "-q",
        queries.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let path = shown(&queries);
    assert!(
        stderr.contains(&format!(
            "{path}:17: query `ByBarcode`: ON CONFLICT ON CONSTRAINT products_barcode_idx names no primary key or unique constraint of `app.products`; its constraints are products_pkey, products_sku_key, uq_code, products_name_code_key"
        )),
        "{stderr}"
    );
    assert!(stderr.contains("1 unsupported query"), "{stderr}");
    Ok(())
}

/// Like PostgreSQL, a default constraint name already taken by another
/// table's constraint in the same schema gets a number (`_pkey1`); one in
/// another schema does not. The expected names are the ones PostgreSQL 17
/// gives this schema.
#[test]
fn on_conflict_on_constraint_resolves_default_names_across_the_schema() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    let queries = dir.path().join("queries.sql");
    let table = "inventory_adjustment_requests_awaiting_regional_review_by";
    fs::write(
        &schema,
        format!(
            "CREATE TABLE cx.{table}_east (id INT PRIMARY KEY, code TEXT UNIQUE);\n\
             CREATE TABLE cx.{table}_west (id INT PRIMARY KEY, code TEXT UNIQUE);\n\
             CREATE TABLE cy.{table}_north (id INT PRIMARY KEY);\n"
        ),
    )?;
    let upsert = |name: &str, target: &str, constraint: &str| {
        format!(
            "-- name: {name} :exec\n\
             INSERT INTO {target} (id) VALUES ($1)\n\
             ON CONFLICT ON CONSTRAINT {constraint} DO NOTHING;\n\n"
        )
    };
    let east = format!("cx.{table}_east");
    let west = format!("cx.{table}_west");
    let north = format!("cy.{table}_north");
    fs::write(
        &queries,
        [
            upsert("EastKey", &east, &format!("{table}__pkey")),
            upsert(
                "EastCode",
                &east,
                "inventory_adjustment_requests_awaiting_regional_review_code_key",
            ),
            upsert("WestKey", &west, &format!("{table}_pkey1")),
            upsert(
                "WestCode",
                &west,
                "inventory_adjustment_requests_awaiting_regional_revie_code_key1",
            ),
            upsert("NorthKey", &north, &format!("{table}__pkey")),
            upsert("WestByEastKey", &west, &format!("{table}__pkey")),
        ]
        .concat(),
    )?;
    let out = output_dir(&dir);

    let output = run_binary(&[
        "g",
        "-s",
        schema.to_str().unwrap(),
        "-q",
        queries.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let path = shown(&queries);
    assert!(
        stderr.contains(&format!(
            "{path}:21: query `WestByEastKey`: ON CONFLICT ON CONSTRAINT {table}__pkey names no primary key or unique constraint of `{west}`; its constraints are {table}_pkey1, inventory_adjustment_requests_awaiting_regional_revie_code_key1"
        )),
        "{stderr}"
    );
    assert!(stderr.contains("1 unsupported query"), "{stderr}");
    Ok(())
}

/// Default constraint names also avoid the names an index, a sequence, or a
/// view already takes in the schema, until it is dropped. The expected names
/// are the ones PostgreSQL 17 gives this schema.
#[test]
fn default_constraint_names_avoid_indexes_sequences_and_views() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    let queries = dir.path().join("queries.sql");
    fs::write(
        &schema,
        "CREATE TABLE a (id INT PRIMARY KEY, code TEXT);\n\
         CREATE INDEX b_pkey ON a (code);\n\
         CREATE TABLE b (id INT PRIMARY KEY, code TEXT UNIQUE);\n\
         CREATE VIEW c_pkey AS SELECT 1 AS one;\n\
         CREATE TABLE c (id INT PRIMARY KEY);\n\
         CREATE SEQUENCE d_code_key;\n\
         CREATE TABLE d (id SERIAL PRIMARY KEY, code TEXT UNIQUE);\n\
         CREATE MATERIALIZED VIEW e_pkey AS SELECT 1 AS one;\n\
         DROP MATERIALIZED VIEW e_pkey;\n\
         CREATE TABLE e (id INT PRIMARY KEY);\n",
    )?;
    let upsert = |name: &str, target: &str, constraint: &str| {
        format!(
            "-- name: {name} :exec\n\
             INSERT INTO {target} (id) VALUES ($1)\n\
             ON CONFLICT ON CONSTRAINT {constraint} DO NOTHING;\n\n"
        )
    };
    fs::write(
        &queries,
        [
            upsert("BKey", "b", "b_pkey1"),
            upsert("BCode", "b", "b_code_key"),
            upsert("CKey", "c", "c_pkey1"),
            upsert("DKey", "d", "d_pkey"),
            upsert("DCode", "d", "d_code_key1"),
            upsert("EKey", "e", "e_pkey"),
        ]
        .concat(),
    )?;
    let out = output_dir(&dir);

    let output = run_binary(&[
        "g",
        "-s",
        schema.to_str().unwrap(),
        "-q",
        queries.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    Ok(())
}

#[test]
fn unique_keys_get_lookups_but_expression_and_partial_indexes_do_not() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, UNIQUE_KEYS_SCHEMA)?;
    let out = output_dir(&dir);
    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ])?;

    let read = fs::read_to_string(out.join("products/read.rs"))?;
    assert!(
        read.contains(
            "pub async fn get_by_sku<'e, E>(executor: E, sku: &str) -> Result<Option<Product>, sqlx::Error>"
        ),
        "{read}"
    );
    assert!(read.contains("        WHERE sku = $1\n"), "{read}");
    assert!(
        read.contains(
            "pub async fn get_by_organization_id_and_code<'e, E>(\n    executor: E,\n    organization_id: uuid::Uuid,\n    code: &str,\n) -> Result<Option<Product>, sqlx::Error>"
        ),
        "{read}"
    );
    assert!(
        read.contains("        WHERE organization_id = $1 AND code = $2\n"),
        "{read}"
    );
    assert!(read.contains("    .bind(organization_id)\n    .bind(code)\n"));
    assert!(!read.contains("get_by_name"), "expression index: {read}");
    assert!(!read.contains("get_by_barcode"), "partial index: {read}");
    Ok(())
}
