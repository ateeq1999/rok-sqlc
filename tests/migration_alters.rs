//! `ALTER TABLE` handling against the real module migrations.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use rok_sqlc::parser::{SchemaSource, parse_sources};

#[test]
fn corpus_foreign_key_added_by_alter_table_is_recorded() -> Result<()> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "modules/quotes_invoices/migrations/0001_create_quotations_invoices_and_payments.up.sql",
    );
    let sql = fs::read_to_string(&path).with_context(|| format!("failed to read {path:?}"))?;
    let parsed = parse_sources(&[SchemaSource {
        path: &path,
        sql: &sql,
    }])?;

    let quotations = parsed
        .schema
        .tables
        .iter()
        .find(|table| table.sql_name == "quotations")
        .with_context(|| format!("quotations was not generated: {:?}", parsed.failures))?;
    let foreign_key = quotations
        .foreign_keys
        .iter()
        .find(|key| key.column == "converted_invoice_id")
        .context("the ALTER TABLE … ADD CONSTRAINT foreign key was not recorded")?;
    assert_eq!(foreign_key.referenced_table, "invoices");
    assert_eq!(foreign_key.referenced_column, "id");
    Ok(())
}

/// Every module's migrations, parsed in filename order, hit no `ALTER TABLE`
/// failure: the corpus only uses supported or ignored-on-purpose statements.
#[test]
fn corpus_migrations_have_no_alter_table_failures() -> Result<()> {
    let modules = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("modules");
    for module in fs::read_dir(&modules)? {
        let migrations = module?.path().join("migrations");
        if !migrations.is_dir() {
            continue;
        }
        let mut paths: Vec<PathBuf> = fs::read_dir(&migrations)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<_>>()?;
        paths.retain(|path| path.to_string_lossy().ends_with(".up.sql"));
        paths.sort();
        let texts: Vec<String> = paths
            .iter()
            .map(fs::read_to_string)
            .collect::<std::io::Result<_>>()?;
        let sources: Vec<SchemaSource<'_>> = paths
            .iter()
            .zip(&texts)
            .map(|(path, sql)| SchemaSource { path, sql })
            .collect();
        let parsed = parse_sources(&sources)?;
        let alter_failures: Vec<String> = parsed
            .failures
            .iter()
            .filter(|failure| failure.reason.contains("ALTER TABLE"))
            .map(ToString::to_string)
            .collect();
        assert!(alter_failures.is_empty(), "{alter_failures:#?}");
    }
    Ok(())
}
