//! Starts the server. Nothing else: the modules are declared in `lib.rs`,
//! wiring lives in `app.rs`, settings in `config.rs`.

use {crate}::{app, config};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let config = config::Config::from_env().map_err(std::io::Error::other)?;
    app::run(config).await
}
