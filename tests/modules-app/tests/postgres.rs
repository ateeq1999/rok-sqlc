//! Applies every module fixture's migrations to a fresh PostgreSQL database
//! and smoke-runs the generated reads of every table.
//!
//! Runs only when `ROK_SQLC_TEST_DATABASE_URL` is set. The test creates its
//! own database on that server (the URL's role needs `CREATEDB`) and drops
//! it afterwards; the database the URL names is not touched.

use std::str::FromStr;

use rok_sqlc_modules_app::db::smoke;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

#[tokio::test]
async fn generated_reads_against_the_migrated_corpus() {
    let Ok(url) = std::env::var("ROK_SQLC_TEST_DATABASE_URL") else {
        eprintln!("skipping: ROK_SQLC_TEST_DATABASE_URL is not set");
        return;
    };
    let admin = PgPool::connect(&url).await.expect("connect to PostgreSQL");
    let name = format!("rok_sqlc_modules_{}", std::process::id());
    let drop_database = format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)");
    sqlx::raw_sql(&drop_database)
        .execute(&admin)
        .await
        .expect("drop leftover database");
    sqlx::raw_sql(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .expect("create fresh database");

    let options = PgConnectOptions::from_str(&url)
        .expect("parse ROK_SQLC_TEST_DATABASE_URL")
        .database(&name);
    let result = match PgPoolOptions::new().connect_with(options).await {
        Ok(pool) => {
            let result = migrate_and_read(&pool).await;
            pool.close().await;
            result
        }
        Err(err) => Err(format!("connect to {name}: {err}")),
    };

    sqlx::raw_sql(&drop_database)
        .execute(&admin)
        .await
        .expect("drop test database");
    admin.close().await;

    if let Err(failures) = result {
        panic!("{failures}");
    }
}

async fn migrate_and_read(pool: &PgPool) -> Result<(), String> {
    for (path, sql) in smoke::MIGRATIONS {
        sqlx::raw_sql(sql)
            .execute(pool)
            .await
            .map_err(|err| format!("migration {path} failed: {err}"))?;
    }
    let failures = smoke::run(pool).await;
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} generated read(s) failed:\n{}",
            failures.len(),
            failures.join("\n")
        ))
    }
}
