//! A consuming crate's `build.rs`, as the README shows it. Compiled with the
//! crate's examples so the README's sample keeps building; run it only from
//! a crate laid out like `docs/examples/quickstart`, since it writes `src/db`.

use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed=migrations");
    println!("cargo::rerun-if-changed=queries.sql");

    // Paths are relative to the package root, where Cargo runs build scripts.
    let mut config = rok_sqlc::Config::default();
    config.schema = vec!["migrations".into()];
    config.queries = vec!["queries.sql".into()];
    config.output = "src/db".into();

    match rok_sqlc::generate(&config) {
        Ok(_report) => Ok(()),
        Err(error) => {
            // The report says which tables and queries failed, and where.
            for failure in &error.report().table_failures {
                println!("cargo::warning={failure}");
            }
            for failure in &error.report().query_failures {
                println!("cargo::warning={failure}");
            }
            Err(error.into())
        }
    }
}
