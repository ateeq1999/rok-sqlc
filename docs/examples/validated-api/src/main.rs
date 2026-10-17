//! Serves the axum routes against a database the migrations have been
//! applied to: `DATABASE_URL=postgres://localhost/members cargo run`.

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL").expect("set DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(listener, rok_sqlc_validated_api::with_axum::router(pool)).await?;
    Ok(())
}
