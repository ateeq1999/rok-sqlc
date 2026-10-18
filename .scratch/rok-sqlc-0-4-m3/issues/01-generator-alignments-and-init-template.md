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

**Status:** ready-for-agent

- [ ] Generator tests (and the expected OpenAPI document) show `minimum: 0` and the new descriptions.
- [ ] CLI tests: `init --template actix` copies the files, skips existing and generated ones, and rejects an unknown name; the packaged-file test includes the template; the README documents the flag (drift tests).
