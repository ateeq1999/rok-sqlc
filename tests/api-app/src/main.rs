//! Starts the server, as the scaffolded `main.rs` does, through the library
//! that declares the skeleton's modules.

use rok_sqlc_api_app::{app, config};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let config = config::Config::from_env().map_err(std::io::Error::other)?;
    app::run(config).await
}
