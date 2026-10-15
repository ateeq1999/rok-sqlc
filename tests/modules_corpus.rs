use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use anyhow::{Result, bail};

use rok_sqlc::parser::parse_schema;

struct Expected {
    module: &'static str,
    tables: &'static [&'static str],
    failures: &'static [&'static str],
}

const CORPUS: &[Expected] = &[
    Expected {
        module: "inventory",
        tables: &[
            "suppliers",
            "categories",
            "warehouses",
            "products",
            "stock_levels",
        ],
        failures: &[],
    },
    Expected {
        module: "accounting",
        tables: &["accounts"],
        failures: &["journal_entries", "ledger_lines"],
    },
    Expected {
        module: "hr",
        tables: &["departments"],
        failures: &["employees"],
    },
    Expected {
        module: "sales",
        tables: &["customers", "orders"],
        failures: &["order_items"],
    },
];

fn modules_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("modules")
}

#[test]
fn module_fixtures_match_expected_outcomes() -> Result<()> {
    for case in CORPUS {
        let schema_path = modules_dir().join(case.module).join("schema.sql");
        let sql = fs::read_to_string(&schema_path)
            .map_err(|e| anyhow::anyhow!("failed to read {schema_path:?}: {e}"))?;
        let parsed = parse_schema(&sql)?;

        let actual_tables: HashSet<_> = parsed
            .schema
            .tables
            .iter()
            .map(|table| table.sql_name.as_str())
            .collect();
        let actual_failures: HashSet<_> = parsed
            .failures
            .iter()
            .map(|failure| failure.table.as_str())
            .collect();

        let expected_tables: HashSet<_> = case.tables.iter().copied().collect();
        if actual_tables != expected_tables {
            bail!(
                "module `{}`: expected tables {expected_tables:?}, got {actual_tables:?}",
                case.module
            );
        }

        let expected_failures: HashSet<_> = case.failures.iter().copied().collect();
        if actual_failures != expected_failures {
            bail!(
                "module `{}`: expected failures {expected_failures:?}, got {actual_failures:?}",
                case.module
            );
        }
    }
    Ok(())
}

#[test]
fn every_module_has_a_query_file() -> Result<()> {
    for case in CORPUS {
        let queries = modules_dir().join(case.module).join("queries.sql");
        let sql = fs::read_to_string(&queries)
            .map_err(|e| anyhow::anyhow!("failed to read {queries:?}: {e}"))?;
        if !sql.contains("-- name:") {
            bail!(
                "module `{}`: queries.sql carries no SQLC annotations",
                case.module
            );
        }
    }
    Ok(())
}
