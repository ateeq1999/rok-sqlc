//! Runs the quickstart against a database the migrations have been applied
//! to: `DATABASE_URL=postgres://localhost/catalog cargo run`.

#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    let url = std::env::var("DATABASE_URL").expect("set DATABASE_URL");
    let pool = sqlx::PgPool::connect(&url).await?;
    rok_sqlc_quickstart::quickstart(&pool).await
}
