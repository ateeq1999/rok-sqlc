# 01: Generator alignments, and `init --template`

**What to build:**

1. **`PaginatedResponse.total`** gets `minimum: 0` in its schema, like the target's `total`, which is `u64`. The other fields stay as they are.
2. **Path parameter descriptions** read like the target. A single key is "<Singular> id" (`users` → "User id", `audit_log` → "Audit log entry id" after a module rename, so use the module's singular name). For composite keys, unique lookups and relation `{other}` params, use the same style: "<Singular> <column words>". Check the target document's wording for its path params and match it.
3. **`rok-sqlc init --template <name>`** (short alias per the CLI tests' rule, if flags need one), with `actix` the only template for now:
   - it copies the template's files from `templates/<name>/` into the project, each only if absent, as scaffolded files are;
   - the template's files are embedded in the binary at compile time, so `init` works from `cargo install`; consider `include_dir` (check that it builds on Rust 1.88) or a small `build.rs`;
   - they are packaged in the published crate (`Cargo.toml` `include`, checked by `tests/release_metadata.rs`);
   - it skips generated files (anything carrying `@generated`), since the user runs `rok-sqlc g`;
   - it prints the next steps;
   - an unknown template lists the valid ones;
   - plain `init` (no `--template`) behaves exactly as today.

   Ticket 02 writes the template itself. For this ticket, create a minimal placeholder `templates/actix-api/` (a `rok-sqlc.toml` and a migration) so the mechanism is testable, and make the embedding pick up whatever ticket 02 adds without code changes.

**Blocked by:** None

**Status:** resolved

- [x] Generator tests (and the expected OpenAPI document) show `minimum: 0` and the new descriptions.
- [x] CLI tests: `init --template actix` copies the files, skips existing and generated ones, and rejects an unknown name; the packaged-file test includes the template; the README documents the flag (drift tests).

## Comments

Done on branch `rok-sqlc-0-4-m3-01`.

What changed:

- `src/generator/api/schemas.rs::describe_response` adds `#[schema(minimum = 0)]` to `PaginatedResponse::total` (still `i64`, so `format: int64`, as in the target).
- `src/generator/api/operation.rs`: `path_param` takes the description; `key_description(table)` is "<Singular> id" and `column_description(table, column)` "<Singular> <column words>", where Singular is the table's struct name as words with the first capitalized (`AuditLogEntry` → "Audit log entry", `ApiKey` → "Api key": the target's "API key" differs only in a description, which the contract ignores). Single key: "User id". Composite key columns and a unique lookup's `{value}`: "Stock level warehouse code", "Account email". A many-to-many `{other}`: the related table's "<Singular> id" ("Tag id"). Tested in `tests/api_paths.rs` and `tests/api_openapi.rs`; `tests/api-app/tests/expected/openapi.json` and `docs/examples/api` regenerated.
- `rok-sqlc init --template <NAME>` / `-t` (`src/init/template.rs`, `src/cli.rs`). Plain `init` is unchanged. With `--template`, only the template is copied (not the built-in skeleton, and no empty `migrations/`): the template is expected to hold its own skeleton. An unknown name is a clap usage error listing the possible values (`[possible values: actix]`); `init_template` itself also errors with the list.

What tickets 02 and 03 must know:

- **Adding template files:** put them under `templates/actix-api/`. `build.rs` walks every directory of `templates/` and embeds every file (`include_bytes!`, sorted, `cargo:rerun-if-changed=templates`), except a template's top-level `target/`. No code changes are needed for new files. The name-to-directory map is `TEMPLATES` in `src/init/template.rs` (`("actix", "actix-api")`); a new template is one entry there.
- **The template's `Cargo.toml` must be checked in as `Cargo.toml.template`.** Cargo leaves any directory holding a `Cargo.toml` (a nested package) out of the published crate, which would silently drop the whole template. `init` copies `<path>.template` as `<path>` (any file, any directory). `tests/release_metadata.rs::package_ships_every_template_file` fails if any template file is missing from `cargo package --list`. To build or test the template in place (CI, fixture harness), copy or symlink `Cargo.toml.template` to `Cargo.toml` in a temp copy, or run `rok-sqlc init --template actix <tmp>` and `rok-sqlc g` there; a `Cargo.toml` inside `templates/actix-api/` must not be committed. Add `Cargo.toml` (and `target/`, `Cargo.lock` if built in place) to a `.gitignore` there if you build in place.
- **What init copies:** every embedded file whose first line does not contain `@generated` (so rok-sqlc's output and `Cargo.lock` are skipped; a README that mentions `@generated` later is still copied), with `\r\n` normalized to `\n`, each only when absent, and cargo's untouched default `src/main.rs` replaced, as plain `init` does. Files must be UTF-8. Nothing is substituted (no `{crate}` placeholder), with one exception: when the target project already has a `Cargo.toml`, `<template_crate>::` in `.rs` files becomes `<project_crate>::`, where the template crate is the lib/package name in `Cargo.toml.template` (`-` as `_`). So write `main.rs` as `use actix_api::{...}` (whatever the template's crate is) and it adapts.
- **Next steps printed** (in `src/cli.rs`, the `template: Some(..)` arm): add dependencies from `rok-sqlc.toml` to `Cargo.toml`, run `rok-sqlc g`, set `DATABASE_URL` and `JWT_SECRET`, then `cargo run`. Adjust the wording there if the template differs (step 1 is moot when the template's `Cargo.toml` was copied).
- **Placeholder files to replace:** `templates/actix-api/rok-sqlc.toml`, `migrations/0001_init.sql`, and `src/db/mod.rs` (a one-line `@generated` file, there so the CLI test sees a generated file being skipped). `tests/cli/init_template.rs` compares the copied tree with the template's non-generated files, so it follows whatever 02 adds; it needs the template to keep at least one `@generated` file and a `rok-sqlc.toml`.
- README: "Starting from a template" under the `init` section documents the flag (the flag table row is drift-checked); 03 may expand it or link it from the guide. CHANGELOG `[Unreleased]` has both changes.
