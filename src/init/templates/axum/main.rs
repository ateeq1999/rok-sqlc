//! Starts the server. Nothing else: the modules are declared in `lib.rs`,
//! wiring lives in `app.rs`, settings in `config.rs`.

use std::process::ExitCode;

use tracing_subscriber::EnvFilter;

use {crate}::{app, config};

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(config::log_filter()))
        .init();
    let started = match config::Config::from_env() {
        Ok(config) => app::run(config).await.map_err(|err| err.to_string()),
        Err(err) => Err(err.to_string()),
    };
    match started {
        Ok(()) => ExitCode::SUCCESS,
        // One readable line, such as "error: environment variable
        // `JWT_SECRET` is not set", and a non-zero exit.
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
