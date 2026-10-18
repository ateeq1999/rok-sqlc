// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Starts the server. Nothing else: wiring lives in `app.rs`, settings in
//! `config.rs`.

mod app;
mod config;
mod db;
mod error;
mod features;
mod shared;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let config = config::Config::from_env().map_err(std::io::Error::other)?;
    app::run(config).await
}
