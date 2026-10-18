# 04: Scaffolded files, and `rok-sqlc init`

**What to build:**

1. **Scaffolded files in the writer** (`src/output/writer.rs`): a scaffolded file is written only when absent. It's never rewritten or reported as changed, and never removed as stale. `--check` reports a missing one as drift, but never its contents. It starts with a header: `// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.` The `Report` lists scaffolded files that were written.

2. **`rok-sqlc init [DIR]`**, a new subcommand. It scaffolds the application skeleton from ADR 0016 into a project (default `.`), each file only if absent:
   - `Cargo.toml` hints, as comments in `rok-sqlc.toml`, or a printed list;
   - `rok-sqlc.toml` with `api.output = "src/features"`, `derive = ["serde", "validate"]`, `extractors = ["actix_web"]`, `primary_key_ids = "uuid_v7"`;
   - `migrations/` (empty);
   - `src/main.rs`, which starts the server and nothing else;
   - `src/app.rs`, which builds state, calls the generated `register_services` and `configure_all`, and mounts health;
   - `src/config.rs` (`Config::from_env`: `DATABASE_URL`, `BIND_ADDR`, `JWT_SECRET`, pool settings), the only env reader;
   - `src/error.rs`: `AppError` with `BadRequest`, `Unauthorized`, `Forbidden`, `NotFound`, `Conflict`, `PayloadTooLarge`, `Validation`, `PreconditionRequired`, `VersionConflict` and `Internal`. It implements `ResponseError` with the wire standard's body and codes (ticket 01's types), plus `From<sqlx::Error>`: unique and foreign-key violations become `Conflict` naming the constraint, check violations `Validation`, `RowNotFound` becomes `NotFound`, everything else `Internal`, logged with `tracing`;
   - `src/shared/mod.rs` and `src/shared/auth.rs`: `AuthUser {id: Uuid, role}` and `AdminUser(AuthUser)` as actix `FromRequest`, verifying an HS256 JWT from `Authorization: Bearer` with `jsonwebtoken`, 401 and 403 through `AppError`, with `Role` as `User`/`Admin` snake_case;
   - `src/features/mod.rs` (`mod generated; pub use generated::*;`);
   - `AGENTS.md`: the rules and where code goes, from `notes/architecture-guidelines-original.md` sections 3 and 4 with ADR 0016's amendments.

   Paths the skeleton refers to that tickets 05 and 06 generate (`crate::features::generated::{register_services, configure_all}`, `crate::shared::{response, pagination, validation, health}`, `crate::db::pool`) are fixed here. Record them in the ticket comments.

Use only `actix-web 4`, `sqlx 0.9`, `jsonwebtoken`, `serde`, `tracing`, `thiserror` and `uuid`. No `unwrap`, `expect` or `panic` (generated actix code must pass those lints). `init` doesn't run generation. It prints the next steps: add migrations, run `rok-sqlc g`, `cargo run`.

**Blocked by:** 01, 03

**Status:** resolved

- [x] Writer tests: written when absent, untouched on a rerun (even when edited), never stale, `--check` behaviour.
- [x] `init` CLI tests: files created, rerun idempotent, existing files kept. A compile test (`cargo check`) of `init` plus generation runs once ticket 06 lands. Until then, a unit-level check of each template's text.
- [x] README documents `init` (drift tests).

## Comments

Resolved on branch `rok-sqlc-0-4-04`.

**Writer.** A planned file is scaffolded when its contents start with `output::SCAFFOLD_HEADER` (`// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.`). So a generator (ticket 06's `service.rs`) only has to begin the text with the header; the plan stays `Vec<(PathBuf, String)>`. Absent: written and listed in `Report::scaffolded` (CLI: `scaffolded X` / `would scaffold X`). Present: left alone, unreported, never refused (even under `--force`). `--check` lists a missing one under `missing`, never under `changed`; it lacks the generated marker, so it's never stale. `output::write_absent` writes absent files for `init`.

**`init [DIR]`** (alias `i`, default `.`): `src/init/mod.rs`, templates in `src/init/templates/` (`include_str!`, packaged through a new `Cargo.toml` include line). Rust files get the header prepended; `rok-sqlc.toml` and `AGENTS.md` carry it in `#` / `<!-- -->`. The Cargo.toml hints are comments at the end of `rok-sqlc.toml` (`jsonwebtoken = { version = "11", features = ["rust_crypto"] }`, sqlx 0.9 with `uuid`, uuid `v7`+`serde`, validator 0.21, thiserror 2, tracing 0.1, actix-web 4) plus the clippy lint table.

**Fixed paths and signatures later tickets must generate:**
- `crate::features::generated` (`api.output/generated.rs`, declared `mod generated; pub use generated::*;`), so `app.rs` imports `crate::features::{configure_all, register_services}`:
  - `pub fn register_services(cfg: &mut actix_web::web::ServiceConfig, pool: &sqlx::PgPool)`
  - `pub fn configure_all(cfg: &mut actix_web::web::ServiceConfig)`
- `src/shared/mod.rs` declares `pub mod auth; pub mod health; pub mod pagination; pub mod response; pub mod tenant; pub mod validation;` (all but `auth` generated by 05).
  - `crate::shared::health::routes(cfg: &mut web::ServiceConfig)` mounts `/health/live` and `/health/ready`; `app.rs` registers `web::Data<PgPool>` as app data for it.
  - `crate::shared::response::{ErrorBody, ErrorPayload}` must exist (re-export of `db::http`'s is fine); `error.rs` builds `ErrorBody { error: ErrorPayload { code, message, details } }`.
- `crate::db::pool`: `PoolSettings { pub max_connections: u32, pub min_connections: u32, pub acquire_timeout: Duration, pub idle_timeout: Duration, pub max_lifetime: Duration }` with `Default`; `pub async fn init_pool(url: &str, settings: &PoolSettings) -> Result<PgPool, sqlx::Error>`; `check_health` is 05's (the skeleton doesn't call it). `config.rs` reads `DB_MAX_CONNECTIONS`, `DB_MIN_CONNECTIONS`, `DB_ACQUIRE_TIMEOUT_SECS`, `DB_IDLE_TIMEOUT_SECS`, `DB_MAX_LIFETIME_SECS` into it.
- `crate::shared::auth::{AuthUser { id: Uuid, role: Role }, AdminUser(pub AuthUser), Role { User, Admin }, Claims { sub, role, exp }, JwtKeys}`; `app.rs` registers `web::Data<JwtKeys>`.

**`crate::error::AppError`** (`thiserror`, `ResponseError`, `#[allow(dead_code)]` so unused variants pass the lint gate): `BadRequest(String)` 400, `Unauthorized(String)` 401, `Forbidden(String)` 403, `NotFound(String)` 404, `Conflict(String)` 409, `PayloadTooLarge(String)` 413, `Validation { message: String, details: Option<validator::ValidationErrors> }` 422, `PreconditionRequired(String)` 428, `VersionConflict(String)` 412, `Internal(Box<dyn std::error::Error + Send + Sync>)` 500 (build with `AppError::Internal("...".into())`; logged with `tracing::error!` when answered, message "something went wrong"). `AppError::code() -> &'static str`. `From<sqlx::Error>`: `RowNotFound` → `NotFound`, unique / foreign-key violation → `Conflict` naming the constraint, check violation → `Validation` (no details), else `Internal`. `From<validator::ValidationErrors>` → `Validation` with details.

**Deviations.** `error.rs` uses `validator` (not in the ticket's crate list) for `Validation`'s details; the generated data layer needs it anyway. AGENTS.md's rules 1 and 2 note the infrastructure exceptions (`app.rs`, `error.rs`, `shared/`; `service.rs` naming `PgPool`/`PgConnection`), which ticket 09 should settle. `init` has the alias `i` (the CLI tests require a single-letter alias per subcommand).

**Compile check.** Left to ticket 06 (needs 05/06's files). Here: unit tests of each template's text (`src/init/tests.rs`). I also compiled the skeleton by hand against stubs of the fixed paths with Rust 1.94 and `clippy -D warnings` plus `unwrap_used`/`expect_used`/`panic` denied, and ran an actix test of `AuthUser`/`AdminUser` (401, 403) and `Internal` (500): all clean.
