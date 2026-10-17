# 06: Query directories: one generated module per query file

**What to build:** `--queries` accepts a file or a directory and can be repeated. A directory of query files generates one module per file, named after the file stem (`queries/users.sql` → `db/queries/users.rs`, declared by a generated `db/queries/mod.rs`). A single file keeps producing `db/queries.rs`. Function names must be unique within a module, not across modules. A query-file stem that isn't a valid Rust module name fails with a diagnostic.

**Blocked by:** 02

**Status:** resolved

- [x] A directory with `users.sql` and `orders.sql` generates both modules, and the fixture crate compiles and runs a function from each against PostgreSQL (PostgreSQL seam, plus CLI seam for the file layout).
- [x] Two files may each define a query with the same name without conflict.
- [x] A single `--queries file.sql` still generates `db/queries.rs`.
- [x] Switching between a single file and a directory leaves no stale module behind (covered once ticket 07 lands; until then, documented).

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
