//! The files a generation run writes, and their content.

use super::*;

#[test]
fn generates_resource_modules() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ])?;

    let out = output_dir(&dir);
    assert!(out.join("users/mod.rs").exists());
    assert!(out.join("users/models.rs").exists());
    assert!(out.join("users/read.rs").exists());
    assert!(out.join("users/write.rs").exists());
    assert!(out.join("mod.rs").exists());
    assert!(out.join("schema.rs").exists());

    let models = fs::read_to_string(out.join("users/models.rs"))?;
    assert!(models.contains("pub struct User {"));
    assert!(models.contains("pub struct UserId(pub uuid::Uuid);"));
    assert!(models.contains("pub id: UserId,"));
    assert!(models.contains("pub email: String,"));

    let read = fs::read_to_string(out.join("users/read.rs"))?;
    assert!(read.contains(
        "        SELECT\n            id,\n            email\n        FROM users\n        WHERE id = $1\n"
    ));

    let write = fs::read_to_string(out.join("users/write.rs"))?;
    assert!(write.contains("pub async fn create<'e, E>(executor: E, input: NewUser)"));
    assert!(write.contains(
        "pub async fn update<'e, E>(\n    executor: E,\n    id: UserId,\n    input: UpdateUser,\n)"
    ));
    assert!(write.contains("pub async fn delete<'e, E>(executor: E, id: UserId)"));

    for generated in [&read, &write] {
        assert!(
            !generated.contains("PgPool"),
            "generated code must use the generic executor, not a concrete pool"
        );
    }

    let db_mod = fs::read_to_string(out.join("mod.rs"))?;
    assert!(db_mod.contains("pub mod users;"));
    assert!(db_mod.contains("\"uuid\""));
    Ok(())
}

#[test]
fn generates_composite_keys_arrays_and_time() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(
        &schema,
        "CREATE TABLE line_items (
            order_id UUID NOT NULL,
            product_id UUID NOT NULL,
            labels TEXT[] NOT NULL,
            weekdays INTEGER[],
            cut_off TIME NOT NULL,
            PRIMARY KEY (order_id, product_id)
        );",
    )?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ])?;

    let out = output_dir(&dir);
    let models = fs::read_to_string(out.join("line_items/models.rs"))?;
    assert!(models.contains("pub labels: Vec<String>,"));
    assert!(models.contains("pub weekdays: Option<Vec<i32>>,"));
    assert!(models.contains("pub cut_off: chrono::NaiveTime,"));

    assert!(models.contains(
        "pub struct LineItemKey {\n    /// The `order_id` column.\n    pub order_id: uuid::Uuid,\n    /// The `product_id` column.\n    pub product_id: uuid::Uuid,\n}"
    ));

    let read = fs::read_to_string(out.join("line_items/read.rs"))?;
    assert!(read.contains("get_by_id<'e, E>(\n    executor: E,\n    key: LineItemKey,\n)"));
    assert!(read.contains("WHERE order_id = $1 AND product_id = $2"));
    assert!(read.contains(".bind(key.order_id)"));
    assert!(read.contains(".bind(key.product_id)"));
    assert!(read.contains("    after: Option<LineItemKey>,\n"));
    Ok(())
}

/// `core.users` and `audit.users` share a name, so only the first one
/// declared is generated (the other's module name collides). A foreign key
/// takes the generated table's typed id only when its schema matches.
#[test]
fn foreign_keys_resolve_to_the_table_in_their_schema() -> Result<()> {
    let generate = |first: &str, second: &str| -> Result<String> {
        let dir = TempDir::new()?;
        let schema = dir.path().join("schema.sql");
        fs::write(
            &schema,
            format!(
                "CREATE TABLE {first}.users (id UUID PRIMARY KEY);
                 CREATE TABLE {second}.users (id UUID PRIMARY KEY);
                 CREATE TABLE posts (
                     id BIGSERIAL PRIMARY KEY,
                     author_id UUID NOT NULL REFERENCES core.users (id),
                     auditor_id UUID REFERENCES audit.users (id)
                 );"
            ),
        )?;
        run(&[
            "generate",
            "--schema",
            schema.to_str().unwrap(),
            "--output",
            output_dir(&dir).to_str().unwrap(),
            "--lenient",
        ])?;
        Ok(fs::read_to_string(
            output_dir(&dir).join("posts/models.rs"),
        )?)
    };

    let core_generated = generate("core", "audit")?;
    assert!(core_generated.contains("use super::super::users::models::UserId;"));
    assert!(core_generated.contains("    pub author_id: UserId,\n"));
    assert!(core_generated.contains("    pub auditor_id: Option<uuid::Uuid>,\n"));

    let audit_generated = generate("audit", "core")?;
    assert!(audit_generated.contains("    pub author_id: uuid::Uuid,\n"));
    assert!(audit_generated.contains("    pub auditor_id: Option<UserId>,\n"));
    Ok(())
}

#[test]
fn generates_write_helpers_with_defaults_and_serial() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(
        &schema,
        "CREATE TABLE events (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            note TEXT,
            is_active BOOLEAN NOT NULL DEFAULT TRUE,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );",
    )?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ])?;

    let out = output_dir(&dir);
    let models = fs::read_to_string(out.join("events/models.rs"))?;
    assert!(models.contains("pub struct NewEvent {"));
    assert!(!models.contains("pub id: Option<i64>"));
    assert!(models.contains("pub name: String,"));
    assert!(models.contains("pub note: Option<String>,"));
    assert!(models.contains("pub is_active: Option<bool>,"));
    assert!(models.contains("pub struct UpdateEvent {"));
    assert!(models.contains("pub note: Option<Option<String>>,"));

    let write = fs::read_to_string(out.join("events/write.rs"))?;
    assert!(!write.contains("columns.push(\"id\")"));
    assert!(write.contains("    columns.push(\"name\");\n"));
    assert!(
        write
            .contains("    if input.is_active.is_some() {\n        columns.push(\"is_active\");\n")
    );
    assert!(write.contains(
        "    if let Some(value) = input.is_active {\n        values.push_bind(value);\n"
    ));
    // `name` is required, so the column list is never empty.
    assert!(!write.contains("DEFAULT VALUES"));
    Ok(())
}

/// A serial column is `NOT NULL` in PostgreSQL even off the primary key: the
/// row holds the value, and the insert input leaves it to the sequence.
/// Once its default is dropped, the input must supply it.
#[test]
fn a_serial_column_is_not_null_and_defaulted_until_its_default_is_dropped() -> Result<()> {
    let dir = TempDir::new()?;
    let migrations = dir.path().join("migrations");
    fs::create_dir_all(&migrations)?;
    fs::write(
        migrations.join("0001_tickets.sql"),
        "CREATE TABLE tickets (
            id UUID PRIMARY KEY,
            number BIGSERIAL,
            position SERIAL,
            rank SMALLSERIAL
        );",
    )?;
    fs::write(
        migrations.join("0002_rank.sql"),
        "ALTER TABLE tickets ALTER COLUMN rank DROP DEFAULT;",
    )?;

    run(&[
        "generate",
        "--schema",
        migrations.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ])?;

    let models = fs::read_to_string(output_dir(&dir).join("tickets/models.rs"))?;
    let row = &models[models.find("pub struct Ticket {").unwrap()..];
    let row = &row[..row.find("\n}").unwrap()];
    assert!(row.contains("pub number: i64,"), "{row}");
    assert!(row.contains("pub position: i32,"), "{row}");
    assert!(row.contains("pub rank: i16,"), "{row}");
    let input = &models[models.find("pub struct NewTicket {").unwrap()..];
    let input = &input[..input.find("\n}").unwrap()];
    // The sequence supplies a serial column, so inserts leave it out.
    assert!(!input.contains("pub number"), "{input}");
    assert!(!input.contains("pub position"), "{input}");
    assert!(input.contains("pub rank: i16,"), "{input}");
    Ok(())
}

#[test]
fn generates_products_fixture_with_singular_names_and_schema_qualified_sql() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    let inventory = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("modules/inventory/schema.sql"),
    )?;
    fs::write(
        &schema,
        inventory.replace("CREATE TABLE products", "CREATE TABLE catalog.products"),
    )?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
        "--resource",
        "products",
    ])?;

    let out = output_dir(&dir);
    let models = fs::read_to_string(out.join("products/models.rs"))?;
    // `--resource` leaves categories and suppliers ungenerated, so the
    // foreign keys to them keep their raw types.
    assert!(models.contains(
        "/// A row of the `catalog.products` table.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct Product {
    /// The `id` column.
    pub id: ProductId,
    /// The `sku` column.
    pub sku: String,
    /// The `name` column.
    pub name: String,
    /// The `category_id` column.
    pub category_id: i64,
    /// The `supplier_id` column.
    pub supplier_id: Option<uuid::Uuid>,
    /// The `price_cents` column.
    pub price_cents: i64,
    /// The `stock_on_hand` column.
    pub stock_on_hand: i32,
    /// The `reorder_level` column.
    pub reorder_level: i16,
    /// The `weight_kg` column.
    pub weight_kg: Option<f64>,
    /// The `discontinued` column.
    pub discontinued: bool,
    /// The `released_on` column.
    pub released_on: Option<chrono::NaiveDate>,
    /// The `created_at` column.
    pub created_at: chrono::DateTime<chrono::Utc>,
}
"
    ));
    assert!(models.contains("pub struct NewProduct {"));
    assert!(models.contains("pub struct UpdateProduct {"));

    let read = fs::read_to_string(out.join("products/read.rs"))?;
    assert!(read.contains(
        "pub async fn get_by_id<'e, E>(executor: E, id: ProductId) -> Result<Option<Product>, sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query_as::<_, Product>(
        r#\"
        SELECT
            id,
            sku,
            name,
            category_id,
            supplier_id,
            price_cents,
            stock_on_hand,
            reorder_level,
            weight_kg,
            discontinued,
            released_on,
            created_at
        FROM catalog.products
        WHERE id = $1
        \"#,
    )
    .bind(id)
    .fetch_optional(executor)
    .await
}
"
    ));

    let write = fs::read_to_string(out.join("products/write.rs"))?;
    assert!(write.contains("new(\"UPDATE catalog.products SET \")"));
    assert!(write.contains("DELETE FROM catalog.products"));
    Ok(())
}

#[test]
fn generates_decimal_columns_with_dependency_hints() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(
        &schema,
        "CREATE TABLE money (id BIGSERIAL PRIMARY KEY, amount NUMERIC(18, 2) NOT NULL, fee DECIMAL);",
    )?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ])?;

    let out = output_dir(&dir);
    let models = fs::read_to_string(out.join("money/models.rs"))?;
    assert!(models.contains("pub amount: rust_decimal::Decimal,"));
    assert!(models.contains("pub fee: Option<rust_decimal::Decimal>,"));

    let db_mod = fs::read_to_string(out.join("mod.rs"))?;
    assert!(db_mod.contains("\"rust_decimal\""));
    assert!(db_mod.contains("rust_decimal = \"1\""));
    // The generated code targets SQLx 0.9, which needs Rust 1.94.
    assert!(db_mod.contains("sqlx = { version = \"0.9\", features = ["));
    assert!(db_mod.contains("Rust 1.94"));
    Ok(())
}

#[test]
fn regeneration_is_idempotent() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(&schema, SIMPLE_SCHEMA)?;

    let out = output_dir(&dir).to_str().unwrap().to_owned();
    let args = [
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        out.as_str(),
    ];
    run(&args)?;
    let models = fs::read_to_string(output_dir(&dir).join("users/models.rs"))?;
    run(&args)?;
    assert_eq!(
        fs::read_to_string(output_dir(&dir).join("users/models.rs"))?,
        models
    );
    Ok(())
}

#[test]
fn snapshot_escapes_raw_string_delimiters() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(
        &schema,
        format!("{SIMPLE_SCHEMA}\n-- note: \"# inside comment\n"),
    )?;

    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output_dir(&dir).to_str().unwrap(),
    ])?;

    let snapshot = fs::read_to_string(output_dir(&dir).join("schema.rs"))?;
    assert!(snapshot.contains("r##\""));
    Ok(())
}

#[test]
fn filtered_runs_merge_root_mod() -> Result<()> {
    let dir = TempDir::new()?;
    let schema = dir.path().join("schema.sql");
    fs::write(
        &schema,
        format!("{SIMPLE_SCHEMA}\nCREATE TABLE posts (id BIGSERIAL PRIMARY KEY);"),
    )?;

    let out = output_dir(&dir).to_str().unwrap().to_owned();
    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        &out,
        "--resource",
        "users",
    ])?;
    run(&[
        "generate",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        &out,
        "--resource",
        "posts",
    ])?;

    let db_mod = fs::read_to_string(output_dir(&dir).join("mod.rs"))?;
    assert!(db_mod.contains("pub mod users;"));
    assert!(db_mod.contains("pub mod posts;"));
    assert!(output_dir(&dir).join("users/models.rs").exists());
    assert!(output_dir(&dir).join("posts/models.rs").exists());
    Ok(())
}

#[test]
fn generates_interval_bytea_and_time_with_time_zone_columns() -> Result<()> {
    let dir = TempDir::new()?;
    fs::write(
        dir.path().join("schema.sql"),
        "CREATE TABLE spans (id BIGSERIAL PRIMARY KEY, length INTERVAL NOT NULL, digest BYTEA UNIQUE);\n\
         CREATE TABLE shifts (id BIGSERIAL PRIMARY KEY, opens TIMETZ NOT NULL);\n",
    )?;
    assert_success(&run_binary_in(
        dir.path(),
        &["g", "-s", "schema.sql", "-o", "spans", "-r", "spans"],
    ));
    let spans = read_tree(&dir.path().join("spans"))?;
    let models = &spans[Path::new("spans/models.rs")];
    assert!(models.contains("pub length: sqlx::postgres::types::PgInterval,"));
    assert!(models.contains("pub digest: Option<Vec<u8>>,"));
    let read = &spans[Path::new("spans/read.rs")];
    assert!(read.contains("(executor: E, digest: &[u8])"), "{read}");
    let filter = &spans[Path::new("spans/filter.rs")];
    assert!(filter.contains("pub length: Vec<Equality<sqlx::postgres::types::PgInterval>>,"));
    assert!(filter.contains("pub digest: Vec<Equality<Vec<u8>>>,"));
    // Neither type needs a crate or an SQLx feature of its own.
    let db_mod = &spans[Path::new("mod.rs")];
    assert!(
        db_mod.contains("features = [\"runtime-tokio\", \"postgres\", \"derive\"] }\n"),
        "{db_mod}"
    );

    assert_success(&run_binary_in(
        dir.path(),
        &["g", "-s", "schema.sql", "-o", "both"],
    ));
    let both = read_tree(&dir.path().join("both"))?;
    assert!(
        both[Path::new("shifts/models.rs")].contains("pub opens: sqlx::postgres::types::PgTimeTz,")
    );
    // `PgTimeTz` decodes and encodes through SQLx's `chrono` feature.
    let db_mod = &both[Path::new("mod.rs")];
    assert!(db_mod.contains(", \"chrono\"] }"), "{db_mod}");
    assert!(db_mod.contains("chrono = \"0.4\""), "{db_mod}");
    Ok(())
}
