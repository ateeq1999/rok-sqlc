// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! `UsersService`: the `users` feature's business rules. Override any
//! [`UsersServiceApi`] method here; the others keep their generated defaults.
//! A port this feature needs from another feature is a trait declared here
//! and implemented in `app.rs`.

use sqlx::PgPool;

use super::service_api::UsersServiceApi;

/// The `users` service, registered as app data by `register_services`.
pub struct UsersService {
    /// The pool the service reads and writes through.
    pub pool: PgPool,
}

impl UsersService {
    /// The service over `pool`, as `register_services` builds it. Give it
    /// what else it needs (a port) here.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        UsersService { pool }
    }
}

impl UsersServiceApi for UsersService {
    fn pool(&self) -> &PgPool {
        &self.pool
    }
}
