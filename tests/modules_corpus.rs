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
        tables: &["accounts", "journal_entries", "ledger_lines"],
        failures: &[],
    },
    Expected {
        module: "hr",
        tables: &["departments"],
        failures: &["employees"],
    },
    Expected {
        module: "sales",
        tables: &["customers", "orders", "order_items"],
        failures: &[],
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

/// Expected query-file outcome for one module fixture.
struct ExpectedQueries {
    module: &'static str,
    /// Generated function names, in file order.
    functions: &'static [&'static str],
    /// Rejected queries with a fragment of the reason.
    rejected: &'static [(&'static str, &'static str)],
}

const QUERY_CORPUS: &[ExpectedQueries] = &[
    ExpectedQueries {
        module: "inventory",
        functions: &[
            "create_product",
            "create_products",
            "update_product_stock",
            "delete_product",
            "list_products_by_category",
            "list_products_offset",
            "upsert_product",
        ],
        rejected: &[],
    },
    ExpectedQueries {
        module: "sales",
        functions: &[
            "create_customer",
            "list_orders_cursor",
            "update_order_status",
        ],
        rejected: &[("CreateOrderWithItems", "contains 4 statements")],
    },
    ExpectedQueries {
        module: "accounting",
        functions: &["create_account", "list_accounts", "deactivate_account"],
        rejected: &[],
    },
    ExpectedQueries {
        module: "hr",
        functions: &["create_department"],
        rejected: &[(
            "ListEmployeesByDepartment",
            "table `employees` is not generatable",
        )],
    },
];

#[test]
fn module_query_files_match_expected_outcomes() -> Result<()> {
    for ExpectedQueries {
        module,
        functions,
        rejected,
    } in QUERY_CORPUS
    {
        let dir = modules_dir().join(module);
        let parsed = parse_schema(&fs::read_to_string(dir.join("schema.sql"))?)?;
        let (defs, mut failures) =
            rok_sqlc::queries::parse_query_file(&fs::read_to_string(dir.join("queries.sql"))?);
        let modules = parsed
            .schema
            .tables
            .iter()
            .map(|table| table.module_name.clone())
            .collect();
        let (analyzed, analysis_failures) = rok_sqlc::queries::analyze_queries(
            &defs,
            &parsed.schema.tables,
            &modules,
            &parsed.failures,
        );
        failures.extend(analysis_failures);

        let actual: Vec<&str> = analyzed
            .iter()
            .map(|query| query.fn_name.as_str())
            .collect();
        if actual != *functions {
            bail!("module `{module}`: expected functions {functions:?}, got {actual:?}");
        }
        if failures.len() != rejected.len() {
            bail!("module `{module}`: expected rejections {rejected:?}, got {failures:?}");
        }
        for (failure, (query, reason)) in failures.iter().zip(rejected.iter()) {
            if failure.query != *query || !failure.reason.contains(reason) {
                bail!(
                    "module `{module}`: expected `{query}` rejected with `{reason}`, got {failure}"
                );
            }
        }
    }
    Ok(())
}
