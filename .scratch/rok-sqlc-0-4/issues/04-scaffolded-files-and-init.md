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

**Status:** ready-for-agent

- [ ] Writer tests: written when absent, untouched on a rerun (even when edited), never stale, `--check` behaviour.
- [ ] `init` CLI tests: files created, rerun idempotent, existing files kept. A compile test (`cargo check`) of `init` plus generation runs once ticket 06 lands. Until then, a unit-level check of each template's text.
- [ ] README documents `init` (drift tests).
