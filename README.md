# rok-sqlc --- Build a SQL-to-SQLx Rust Code Generator

A practical first implementation plan for `rok-sqlc`: a Rust CLI that
reads a SQL schema file, discovers tables, and generates SQLx models and
CRUD functions into `db/<resource>/`.

The first version intentionally supports a constrained SQL subset. It
should fail with a clear error on unsupported schema constructs rather
than silently generate incorrect Rust. This is a schema-driven CRUD
generator inspired by SQLC; it is not yet a full SQLC replacement
because SQLC also generates functions from separately authored query
files.

## 1. Goals and non-goals

### Goals

-   CLI powered by `clap`.
-   Input: a schema/migration `.sql` file.
-   Output: one Rust module per table/resource.
-   Generate SQLx-compatible async functions for reads, inserts,
    updates, deletes, and transactions.
-   Provide `--resource`, `--output`, and `--dry-run` options.
-   Keep parsing, type mapping, rendering, and filesystem output
    separate.
-   Use deterministic output and avoid overwriting unrelated files.

### Initial limitations

-   Start with PostgreSQL DDL and SQLx's `postgres` feature.
-   Support straightforward `CREATE TABLE` statements, common scalar
    types (including `NUMERIC`/`DECIMAL` as `rust_decimal::Decimal`),
    array columns of those scalars, primary keys (single or composite),
    nullability, and simple defaults.
-   Generate `get_by_id` with one parameter per primary key column
    (single-column and composite keys alike); `update` and `delete`
    take the same key parameters.
-   Do not attempt to infer arbitrary business queries from schema
    alone.
-   Preserve the input schema as a migration artifact; do not
    automatically execute it against a database.

## 2. CLI UX

``` bash
# Generate code for every supported table in schema.sql
rok-sqlc generate --schema ./schema.sql

# Choose output folder
rok-sqlc generate --schema ./schema.sql --output ./src/db

# Generate only one table/resource
rok-sqlc generate --schema ./schema.sql --output ./src/db --resource users

# Show planned output without writing files
rok-sqlc generate --schema ./schema.sql --output ./src/db --dry-run

# Also generate typed functions for annotated queries (db/queries.rs)
rok-sqlc generate --schema ./schema.sql --queries ./queries.sql --output ./src/db

# Print version/help
rok-sqlc --help
rok-sqlc generate --help
```

Suggested defaults: - `--schema`: required path to a `.sql` file. -
`--output`: `src/db`. - `--resource`: optional table name; if omitted,
generate all supported tables. - `--dry-run`: list files that would be
written. - `--force`: optional future flag to overwrite generated files.
For the first version, generated files may be overwritten only inside
the selected output directory, with a warning.

## 3. Generated layout

For `users` and `posts` tables:

``` text
src/db/
├── mod.rs
├── schema.rs
├── search.rs
├── queries.rs        # only with --queries
├── users/
│   ├── mod.rs
│   ├── filter.rs
│   ├── models.rs
│   ├── read.rs
│   └── write.rs
└── posts/
    ├── mod.rs
    ├── filter.rs
    ├── models.rs
    ├── read.rs
    └── write.rs
```

Responsibilities:

  -----------------------------------------------------------------------
  File                                Responsibility
  ----------------------------------- -----------------------------------
  `db/mod.rs`                         Expose generated resource modules

  `db/schema.rs`                      Verbatim snapshot of the input
                                      schema; never executed

  `db/search.rs`                      Shared `Page`, `Sort`, and typed
                                      filter conditions (`Ordered`,
                                      `Text`, `Equality`)

  `db/queries.rs`                     Functions for annotated queries
                                      (`--queries`, ADR 0008)

  `filter.rs`                         `XFilter` (per-column conditions
                                      and null checks) and `XSortKey`

  `models.rs`                         Structs mapped from table columns
                                      (`X`, `NewX`, `UpdateX`; `X` is the
                                      singular table name)

  `read.rs`                           `get_by_id`, `exists_by_id`, `list`,
                                      `list_page`, `count`, `search`, and
                                      `search_after` helpers

  `write.rs`                          `create`, `update`, and `delete`
                                      helpers
  -----------------------------------------------------------------------

The shipped generator emits exactly the files above. It does **not**
generate a `pool.rs`/`error.rs` (pool and error ownership stay with the
application) or per-resource `tx.rs`/`migration.rs` files (transactions
are handled by the generic executor described below, and the schema
snapshot lives in `schema.rs`). Sections that show those extra files do
so to illustrate the incremental design, not the current output.

**Important:** avoid generating a second pool configuration if the
parent application already owns pool lifecycle. In that case, generated
functions take a generic `sqlx::PgExecutor`, so callers pass `&PgPool` or
`&mut *tx`.

## 4. Create the project

``` bash
cargo new rok-sqlc
cd rok-sqlc
```

`Cargo.toml`:

``` toml
[package]
name = "rok-sqlc"
version = "0.1.0"
edition = "2024"
description = "Generate SQLx Rust database modules from SQL schema files"
license = "MIT OR Apache-2.0"

[[bin]]
name = "rok-sqlc"
path = "src/main.rs"

[dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive"] }
heck = "0.5"
sqlparser = "0.59"
thiserror = "2"

[dev-dependencies]
tempfile = "3"
```

Use versions compatible with the Rust toolchain you support; update the
`sqlparser` version as needed. The generated application---not
necessarily the CLI itself---needs SQLx:

``` toml
sqlx = { version = "0.8", features = [
  "runtime-tokio",
  "postgres",
  "derive",
  "macros",
  "uuid",
  "chrono",
  "json"
] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Only enable SQLx features actually needed by your application. If
generated code uses `query_as!`, SQLx compile-time query checking
requires database metadata or a configured database; this guide uses
`query_as`/`query` initially to keep generation independent of a live
database.

## 5. CLI entry point

Create `src/main.rs`:

``` rust
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod generate;
mod parser;
mod render;
mod types;

#[derive(Debug, Parser)]
#[command(
    name = "rok-sqlc",
    version,
    about = "Generate SQLx Rust modules from SQL schema files"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate Rust database modules from a schema file
    Generate {
        /// Input schema/migration SQL file
        #[arg(short, long, value_name = "FILE")]
        schema: PathBuf,

        /// Output directory (default: src/db)
        #[arg(short, long, default_value = "src/db", value_name = "DIR")]
        output: PathBuf,

        /// Generate only one table/resource
        #[arg(short, long, value_name = "TABLE")]
        resource: Option<String>,

        /// Print files that would be generated without writing them
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Generate {
            schema,
            output,
            resource,
            dry_run,
        } => {
            let sql = std::fs::read_to_string(&schema)
                .with_context(|| format!("failed to read schema: {}", schema.display()))?;

            let parsed = parser::parse_schema(&sql)
                .with_context(|| format!("failed to parse SQL schema: {}", schema.display()))?;

            generate::generate(&parsed, &schema, &output, resource.as_deref(), dry_run)?;
        }
    }

    Ok(())
}
```

## 6. Internal data model

Create `src/types.rs`:

``` rust
#[derive(Debug, Clone)]
pub struct Schema {
    pub tables: Vec<Table>,
}

#[derive(Debug, Clone)]
pub struct Table {
    /// Original SQL table name, e.g. `user_profiles`.
    pub sql_name: String,
    /// Rust module name, e.g. `user_profiles`.
    pub module_name: String,
    /// Rust type name, e.g. `UserProfile`.
    pub struct_name: String,
    pub columns: Vec<Column>,
    /// Primary key columns in declaration order; empty when the table has no primary key.
    pub primary_key: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Column {
    pub sql_name: String,
    pub rust_name: String,
    pub rust_type: String,
    pub sqlx_type: String,
    pub nullable: bool,
    pub is_primary_key: bool,
}
```

## 7. Parse SQL schema

Use `sqlparser` instead of splitting SQL on semicolons or parsing it
with regular expressions. SQL syntax has quoted identifiers, comments,
nested expressions, and dialect-specific details.

Create `src/parser.rs`:

``` rust
use anyhow::{bail, Context, Result};
use heck::{ToSnakeCase, ToUpperCamelCase};
use sqlparser::{
    ast::{ColumnOption, DataType, Statement, TableConstraint},
    dialect::PostgreSqlDialect,
    parser::Parser,
};

use crate::types::{Column, Schema, Table};

pub fn parse_schema(sql: &str) -> Result<Schema> {
    let dialect = PostgreSqlDialect {};
    let statements = Parser::parse_sql(&dialect, sql)
        .context("invalid PostgreSQL SQL syntax")?;

    let mut tables = Vec::new();

    for statement in statements {
        let Statement::CreateTable(create) = statement else {
            // Ignore non-CREATE TABLE statements in v1. A future strict mode
            // can warn or reject unknown statements.
            continue;
        };

        let sql_name = create.name.to_string().replace('"', "");
        let table_leaf = sql_name
            .rsplit('.')
            .next()
            .unwrap_or(&sql_name)
            .to_owned();

        let mut columns = Vec::new();
        let mut table_primary_key: Option<Vec<String>> = None;

        for constraint in &create.constraints {
            if let TableConstraint::PrimaryKey { columns, .. } = constraint {
                table_primary_key = Some(
                    columns.iter().map(|ident| ident.value.clone()).collect()
                );
            }
        }

        for column_def in &create.columns {
            let column_name = column_def.name.value.clone();
            let nullable = !column_def.options.iter().any(|option| {
                matches!(option.option, ColumnOption::NotNull)
            });

            let inline_primary_key = column_def.options.iter().any(|option| {
                matches!(option.option, ColumnOption::Unique { is_primary: true, .. })
            });

            let (rust_type, sqlx_type) = map_type(&column_def.data_type)
                .with_context(|| format!(
                    "unsupported type for column `{}.{}`",
                    table_leaf, column_name
                ))?;

            columns.push(Column {
                rust_name: column_name.to_snake_case(),
                sql_name: column_name,
                rust_type: if nullable {
                    format!("Option<{rust_type}>")
                } else {
                    rust_type
                },
                sqlx_type,
                nullable,
                is_primary_key: inline_primary_key,
            });
        }

        let primary_columns = table_primary_key.unwrap_or_default();
        let primary_key = if primary_columns.is_empty() {
            columns.iter().filter(|column| column.is_primary_key)
                .map(|column| column.sql_name.clone())
                .collect::<Vec<_>>()
        } else {
            primary_columns
        };

        for column in &mut columns {
            if primary_key.contains(&column.sql_name) {
                column.is_primary_key = true;
            }
        }

        tables.push(Table {
            module_name: table_leaf.to_snake_case(),
            struct_name: table_leaf.to_upper_camel_case(),
            sql_name: table_leaf,
            columns,
            primary_key,
        });
    }

    if tables.is_empty() {
        bail!("no supported CREATE TABLE statements found");
    }

    Ok(Schema { tables })
}

fn map_type(data_type: &DataType) -> Result<(String, String)> {
    let normalized = data_type.to_string().to_ascii_lowercase();

    // Return (Rust type, SQLx/Postgres-compatible Rust type).
    // Add mappings only when the generated application's SQLx features support them.
    let mapped = match normalized.as_str() {
        "smallint" | "int2" => ("i16", "i16"),
        "integer" | "int" | "int4" | "serial" => ("i32", "i32"),
        "bigint" | "int8" | "bigserial" => ("i64", "i64"),
        "boolean" | "bool" => ("bool", "bool"),
        "real" | "float4" => ("f32", "f32"),
        "double precision" | "float8" => ("f64", "f64"),
        "text" | "varchar" | "character varying" | "char" | "character" => {
            ("String", "String")
        }
        "uuid" => ("uuid::Uuid", "uuid::Uuid"),
        "date" => ("chrono::NaiveDate", "chrono::NaiveDate"),
        "timestamp" | "timestamp without time zone" => {
            ("chrono::NaiveDateTime", "chrono::NaiveDateTime")
        }
        "timestamp with time zone" | "timestamptz" => {
            ("chrono::DateTime<chrono::Utc>", "chrono::DateTime<chrono::Utc>")
        }
        "json" | "jsonb" => ("serde_json::Value", "serde_json::Value"),
        _ => {
            anyhow::bail!("type `{normalized}` is not supported yet")
        }
    };

    Ok((mapped.0.to_owned(), mapped.1.to_owned()))
}
```

### Parser implementation notes

This is a starter, not a production-ready SQL parser adapter. Before
relying on it, add tests for: - Inline primary keys and table-level
primary keys. - `NOT NULL`, nullable columns, defaults, quoted names,
and schema-qualified table names. - `CREATE TABLE IF NOT EXISTS`. -
`varchar(255)`, `numeric(12, 2)`, arrays, enums, identity columns, and
generated columns. - Foreign keys and composite primary keys.

In particular, parameterized data types such as `VARCHAR(255)` may
stringify differently than the exact names above. Prefer matching the
`DataType` enum variants and extracting relevant parameters rather than
relying on `to_string()` for every type. The parser must correctly
detect inline primary-key options and reject unsupported or ambiguous
constructs.

## 8. Rendering and generated files

Use a renderer that turns parsed tables into strings. Keep rendering
separate from parsing so templates can evolve without changing schema
analysis.

> **Executor note.** For brevity the starter renderer below takes a
> concrete `&PgPool`. The shipped generator instead takes a generic
> `sqlx::PgExecutor<'e>`, so the same function accepts a pool or a
> caller-owned transaction (see §12). Keep that in mind as you read the
> `use sqlx::PgPool;` / `pool: &PgPool` forms in this section.

Create `src/render.rs`:

``` rust
use crate::types::Table;

pub fn render_models(table: &Table) -> String {
    let mut output = String::from(
        "// @generated by rok-sqlc. Do not edit by hand.\n\n"
    );

    for column in &table.columns {
        if column.rust_type.contains("uuid::Uuid") {
            output.push_str("use uuid;\n");
            break;
        }
    }
    for column in &table.columns {
        if column.rust_type.contains("chrono::") {
            output.push_str("use chrono;\n");
            break;
        }
    }
    for column in &table.columns {
        if column.rust_type.contains("serde_json::") {
            output.push_str("use serde_json;\n");
            break;
        }
    }

    output.push_str(&format!(
        "\n#[derive(Debug, Clone, sqlx::FromRow)]\npub struct {} {{\n",
        table.struct_name
    ));

    for column in &table.columns {
        output.push_str(&format!(
            "    pub {}: {},\n",
            column.rust_name, column.rust_type
        ));
    }

    output.push_str("}\n");
    output
}

pub fn render_read(table: &Table) -> String {
    let mut output = String::from(
        "// @generated by rok-sqlc. Do not edit by hand.\n\n"
    );

    output.push_str("use sqlx::PgPool;\n");
    output.push_str("use super::models::*;\n\n");

    if let Some(pk) = &table.primary_key {
        if let Some(column) = table.columns.iter().find(|c| &c.sql_name == pk) {
            output.push_str(&format!(
                "pub async fn get_by_id(pool: &PgPool, id: {}) -> Result<Option<{}>, sqlx::Error> {{\n",
                column.sqlx_type, table.struct_name
            ));
            output.push_str(&format!(
                "    sqlx::query_as::<_, {}>(\"SELECT * FROM {} WHERE {} = $1\")\n",
                table.struct_name, table.sql_name, pk
            ));
            output.push_str("        .bind(id)\n");
            output.push_str("        .fetch_optional(pool)\n");
            output.push_str("        .await\n");
            output.push_str("}\n\n");
        }
    }

    output.push_str(&format!(
        "pub async fn list(pool: &PgPool) -> Result<Vec<{}>, sqlx::Error> {{\n",
        table.struct_name
    ));
    output.push_str(&format!(
        "    sqlx::query_as::<_, {}>(\"SELECT * FROM {}\")\n",
        table.struct_name, table.sql_name
    ));
    output.push_str("        .fetch_all(pool)\n");
    output.push_str("        .await\n");
    output.push_str("}\n");

    output
}

pub fn render_write(table: &Table) -> String {
    let mut output = String::from(
        "// @generated by rok-sqlc. Do not edit by hand.\n\n"
    );
    output.push_str("use sqlx::PgPool;\n\n");
    output.push_str("// Insert/update generation is emitted only when column and key\n");
    output.push_str("// semantics are known. Implement this renderer incrementally.\n");

    if table.primary_key.is_none() {
        output.push_str("// No primary key detected; update/delete helpers omitted.\n");
    }

    output
}

pub fn render_tx(table: &Table) -> String {
    format!(
        "// @generated by rok-sqlc. Do not edit by hand.\n\
         // Transaction helpers for `{}` can be added here.\n\
         use sqlx::{{Postgres, Transaction}};\n\
         \n\
         pub async fn example_transaction_scope(\n\
             _tx: &mut Transaction<'_, Postgres>,\n\
         ) -> Result<(), sqlx::Error> {{\n\
             Ok(())\n\
         }}\n",
        table.sql_name
    )
}

pub fn render_migration(schema_sql: &str) -> String {
    format!(
        "// @generated by rok-sqlc. Do not edit by hand.\n\
         // Original schema snapshot. This does not execute migrations.\n\
         pub const SCHEMA_SQL: &str = r#\"{}\"#;\n",
        schema_sql.replace("\"#", "\" #")
    )
}

pub fn render_resource_mod() -> String {
    "pub mod models;\npub mod read;\npub mod write;\npub mod tx;\npub mod migration;\n".to_owned()
}
```

### Important renderer hardening

Before production use, improve this renderer:

1.  **Quote SQL identifiers.** Never interpolate arbitrary table/column
    names directly into SQL. Add a dialect-aware identifier-quoting
    helper and test reserved words and quoted identifiers.
2.  **Escape migration string content correctly.** A raw string
    delimiter can occur in input. Choose a delimiter not present in the
    schema or emit an escaped regular string literal.
3.  **Use explicit column lists.** `SELECT *` can break when table
    columns change. Generate the ordered column list from the parsed
    schema.
4.  **Use the right insert semantics.** Omit generated/identity columns
    from inserts and decide whether defaults should be applied by the
    database.
5.  **Handle renamed fields.** SQL names and Rust identifiers can
    differ; generate explicit SQL aliases if necessary.
6.  **Do not generate fake CRUD functions.** If key/type/column
    semantics are unsupported, report that resource as unsupported and
    explain why.

## 9. Generate files to disk

Create `src/generate.rs`:

``` rust
use anyhow::{bail, Context, Result};
use std::{fs, path::{Path, PathBuf}};

use crate::{
    render,
    types::{Schema, Table},
};

pub fn generate(
    schema: &Schema,
    schema_path: &Path,
    output: &Path,
    resource: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let schema_sql = fs::read_to_string(schema_path)
        .with_context(|| format!("failed to read {}", schema_path.display()))?;

    let selected: Vec<&Table> = schema.tables.iter()
        .filter(|table| resource.is_none_or(|name| {
            table.sql_name == name || table.module_name == name
        }))
        .collect();

    if selected.is_empty() {
        bail!("no matching resource found for {:?}", resource);
    }

    let mut files: Vec<(PathBuf, String)> = Vec::new();

    for table in &selected {
        let dir = output.join(&table.module_name);
        files.push((dir.join("mod.rs"), render::render_resource_mod()));
        files.push((dir.join("models.rs"), render::render_models(table)));
        files.push((dir.join("read.rs"), render::render_read(table)));
        files.push((dir.join("write.rs"), render::render_write(table)));
        files.push((dir.join("tx.rs"), render::render_tx(table)));
        files.push((
            dir.join("migration.rs"),
            render::render_migration(&schema_sql),
        ));
    }

    let root_mod = selected.iter()
        .map(|table| format!("pub mod {};\n", table.module_name))
        .collect::<String>();
    files.push((output.join("mod.rs"), root_mod));

    for (path, contents) in files {
        if dry_run {
            println!("would write {}", path.display());
            continue;
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        fs::write(&path, contents)
            .with_context(|| format!("failed to write {}", path.display()))?;

        println!("generated {}", path.display());
    }

    Ok(())
}
```

`Option::is_none_or` requires a sufficiently recent Rust toolchain. If
you need an older MSRV, replace it with
`resource.map_or(true, |name| ...)`.

This initial example writes a schema snapshot into each resource's
`migration.rs`. A better design is to write the original schema once, or
generate per-resource DDL from the AST. Do not label a schema snapshot
as a versioned migration system.

## 10. Example schema

`schema.sql`:

``` sql
CREATE TABLE users (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL,
    display_name TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE posts (
    id BIGSERIAL PRIMARY KEY,
    author_id UUID NOT NULL,
    title TEXT NOT NULL,
    body TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

Generated `users/models.rs`:

``` rust
// @generated by rok-sqlc. Do not edit by hand.

/// A row of the `users` table.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct User {
    pub id: uuid::Uuid,
    pub email: String,
    pub display_name: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Input for `create`: every column the caller may supply.
///
/// `Option` fields are columns with a database default: `None` leaves the
/// column out of the `INSERT` so the default applies.
#[derive(Debug, Clone, PartialEq)]
pub struct NewUser {
    pub id: uuid::Uuid,
    pub email: String,
    pub display_name: Option<String>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Partial update for `update`: `None` leaves a column unchanged.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UpdateUser {
    pub email: Option<String>,
    /// `Some(None)` sets the column to `NULL`.
    pub display_name: Option<Option<String>>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl UpdateUser {
    /// Returns `true` when the patch changes no column.
    pub fn is_empty(&self) -> bool {
        self.email.is_none() && self.display_name.is_none() && self.created_at.is_none()
    }
}
```

Generated `get_by_id` from `users/read.rs` (`read.rs` also provides
`exists_by_id`, `list`, `list_page`, and `count`):

``` rust
/// Fetches the row with the given primary key, or `None` if there is none.
pub async fn get_by_id<'e, E>(executor: E, id: uuid::Uuid) -> Result<Option<User>, sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query_as::<_, User>(
        r#"
        SELECT
            id,
            email,
            display_name,
            created_at
        FROM users
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(executor)
    .await
}
```

Generated `update` from `users/write.rs`. Statements whose shape depends
on the input are built with `sqlx::QueryBuilder`, so every placeholder is
allocated by `push_bind` as its value is bound (see ADR 0007):

``` rust
/// Applies a partial update and returns the updated row, or `None` if no row has
/// the given primary key. An empty patch issues no `UPDATE` and returns the
/// current row.
pub async fn update<'e, E>(
    executor: E,
    id: uuid::Uuid,
    input: UpdateUser,
) -> Result<Option<User>, sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    if input.is_empty() {
        return super::read::get_by_id(executor, id).await;
    }

    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("UPDATE users SET ");
    let mut assignments = query.separated(", ");
    if let Some(value) = input.email {
        assignments.push("email = ");
        assignments.push_bind_unseparated(value);
    }
    if let Some(value) = input.display_name {
        assignments.push("display_name = ");
        assignments.push_bind_unseparated(value);
    }
    if let Some(value) = input.created_at {
        assignments.push("created_at = ");
        assignments.push_bind_unseparated(value);
    }
    query.push(" WHERE id = ");
    query.push_bind(id);
    query.push(
        r#"
        RETURNING
            id,
            email,
            display_name,
            created_at
        "#,
    );
    query
        .build_query_as::<User>()
        .fetch_optional(executor)
        .await
}
```

The exact generated types must reflect the parser's actual type mapping.
In particular, `BIGSERIAL` maps to `i64`, UUID columns map to
`uuid::Uuid`, and nullable columns map to `Option<T>`.

## 11. Better SQLC-like workflow: schema + query files

> **Implemented (ADR 0008).** Pass `--queries queries.sql` to generate
> `db/queries.rs`. Supported return modes are `:one` (`fetch_one`),
> `:maybe_one` (`fetch_optional`), `:many`, `:exec`, and `:execrows`.
> Parameter and result types are inferred from the schema where the SQL
> determines them; elsewhere, declare them with `-- param: $N name Type`
> or `-- column: name Type`. Untypable queries fail with a diagnostic
> instead of being guessed. The design notes below predate the
> implementation.

After schema-based CRUD generation works, add query files:

``` text
db/
├── schema/
│   └── users.sql
└── queries/
    ├── users.sql
    └── posts.sql
```

Example `db/queries/users.sql`:

``` sql
-- name: GetUserByEmail :one
SELECT id, email, display_name, created_at
FROM users
WHERE email = $1;

-- name: ListUsers :many
SELECT id, email, display_name, created_at
FROM users
ORDER BY created_at DESC
LIMIT $1 OFFSET $2;
```

The query parser can interpret annotations: - `:one` → `fetch_one` or
`fetch_optional`, based on a chosen annotation convention. - `:many` →
`fetch_all`. - `:exec` → `execute`. - `:execrows` → execute and return
affected-row count.

A real query-driven generator must parse each query, determine bind
parameters and result columns, and map them to Rust types. SQL parser
ASTs help, but accurate inference often requires schema-aware analysis.
Do not guess parameter types when the SQL is ambiguous; allow explicit
overrides in a config file.

## 12. Transaction generation

Generated CRUD functions take their executor generically
(`E: sqlx::PgExecutor`) rather than a concrete `&PgPool`, so the same
function accepts a pool or a caller-owned transaction. The caller decides
the transaction boundary and owns `commit()` / `rollback()`; rok-sqlc
never opens or commits a transaction implicitly.

``` rust
// pool
db::users::read::get_by_id(&pool, id).await?;

// caller-owned transaction
let mut tx = pool.begin().await?;
db::users::write::delete(&mut *tx, id).await?;
tx.commit().await?;
```

Application-specific transaction functions (for example
`mark_user_active`) are not schema-derived; they belong to the query-file
phase, where SQLC-style annotations describe them.

``` rust
use sqlx::{Postgres, Transaction};

pub async fn mark_user_active(
    tx: &mut Transaction<'_, Postgres>,
    user_id: uuid::Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET active = TRUE WHERE id = $1")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;

    Ok(())
}
```

The caller owns `commit()` or `rollback()`.

## 13. Testing strategy

Add unit and integration tests before expanding SQL support.

-   Parser tests: valid table DDL, nullable fields, primary keys,
    unsupported types, composite keys.
-   Naming tests: `user_profiles` → module `user_profiles`, struct
    `UserProfiles`.
-   Renderer tests: deterministic output, correct nullable types, safe
    identifier handling.
-   CLI tests: missing input, invalid SQL, unknown resource,
    `--dry-run`, output directory creation.
-   Compile tests: generate a small fixture crate and run `cargo check`
    against its SQLx dependencies.
-   Database integration tests: use a temporary PostgreSQL instance to
    verify CRUD and transaction behavior.

Example unit test shape:

``` rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_table() {
        let schema = parse_schema(
            "CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);"
        ).expect("schema should parse");

        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.tables[0].sql_name, "users");
        assert_eq!(schema.tables[0].primary_key.as_deref(), Some("id"));
    }
}
```

## 14. Recommended implementation phases

1.  **Phase 1 --- CLI skeleton:** Clap commands, read input, output
    paths, dry-run.
2.  **Phase 2 --- AST parsing:** PostgreSQL `CREATE TABLE`, names,
    columns, nullability, primary keys.
3.  **Phase 3 --- Models:** Generate structs and test generated code
    compiles.
4.  **Phase 4 --- Reads:** Generate `get_by_id` and `list` with explicit
    column lists.
5.  **Phase 5 --- Writes:** Add inserts, updates, and deletes only for
    supported key/default semantics.
6.  **Phase 6 --- Transactions:** Functions accepting caller-owned SQLx
    transactions.
7.  **Phase 7 --- Query files:** Add SQLC-style annotations and
    query-to-function generation.
8.  **Phase 8 --- Production hardening:** identifier quoting,
    deterministic output, schema drift checks, diagnostics, fixtures,
    and CI.

## 15. Definition of done for v1

-   `rok-sqlc generate --schema schema.sql --output src/db` generates a
    module for every supported table.
-   `--resource users` limits generation to `users`.
-   `--dry-run` writes no files.
-   Invalid SQL and unsupported types produce actionable errors.
-   Generated Rust compiles in a fixture project with the documented
    SQLx features.
-   No database connection is required to generate code.
-   Generated output is deterministic for the same input.
-   Unsupported primary keys, types, and DDL are reported instead of
    producing misleading functions.

## 16. Design recommendation for Rok

Keep `rok-sqlc` as a standalone CLI crate with these internal modules:

``` text
src/
├── main.rs
├── cli.rs
├── parser/
│   ├── mod.rs
│   ├── postgres.rs
│   └── diagnostics.rs
├── schema/
│   ├── mod.rs
│   ├── table.rs
│   └── types.rs
├── generator/
│   ├── mod.rs
│   ├── models.rs
│   ├── read.rs
│   ├── write.rs
│   ├── tx.rs
│   └── migration.rs
└── output/
    ├── mod.rs
    └── writer.rs
```

This separation makes it possible to add MySQL or SQLite dialects,
custom naming rules, templates, query annotations, and a `rok-sqlc.toml`
config without coupling all logic to `main.rs`.

**Core principle:** schema files generate schema-aware types and
baseline CRUD. Explicit query files generate application-specific SQL
functions. Keeping those two responsibilities separate will make
`rok-sqlc` easier to trust, test, and evolve.
