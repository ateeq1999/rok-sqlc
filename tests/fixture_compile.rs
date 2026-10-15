use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, ensure};

const EXTRA_SCHEMA: &str = r#"
CREATE TABLE feature_line_items (
    order_id UUID NOT NULL,
    product_id UUID NOT NULL,
    labels TEXT[] NOT NULL,
    weekdays INTEGER[],
    reporter_ids UUID[],
    cut_off TIME,
    unit_price NUMERIC(18,2) NOT NULL,
    PRIMARY KEY (order_id, product_id)
);
"#;

#[test]
fn inventory_module_generates_compiling_rust() -> Result<()> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let schema_path = manifest_dir.join("modules/inventory/schema.sql");
    let schema = fs::read_to_string(&schema_path)
        .with_context(|| format!("failed to read {}", schema_path.display()))?;
    let schema = format!("{schema}\n{EXTRA_SCHEMA}");

    let parsed = rok_sqlc::parser::parse_schema(&schema)?;
    ensure!(
        parsed.failures.is_empty(),
        "inventory fixture must be fully supported, got failures: {:?}",
        parsed.failures
    );

    let app_dir = manifest_dir.join("tests/fixture-app");
    let files =
        rok_sqlc::generator::plan_files(&schema, &parsed.schema.tables, &app_dir.join("src/db"));
    rok_sqlc::output::apply_files(&files, false, true)?;

    let status = Command::new("cargo")
        .args(["check", "--quiet"])
        .current_dir(&app_dir)
        .env(
            "CARGO_TARGET_DIR",
            manifest_dir.join("target/fixture-target"),
        )
        .status()
        .context("failed to run cargo check in tests/fixture-app")?;
    ensure!(status.success(), "generated code failed cargo check");

    Ok(())
}
