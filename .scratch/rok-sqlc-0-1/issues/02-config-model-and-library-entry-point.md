# 02: Shared configuration model and the stable `rok_sqlc::generate(&Config)` entry point

**What to build:** One configuration model (schema inputs, query inputs, output directory, resource filter, dry-run/force/lenient) and one public library function, `generate(&Config) -> Result<Report>`, that does everything the CLI does. The CLI becomes a thin layer that builds a `Config` and calls it. A `build.rs` can call the same function and get the same files and the same failures. Everything else in the library becomes hidden or crate-private, and that API carries no stability promise. Record the library contract (one stable door, generated-API changes are breaking, SQLx 0.8 only, Rust 1.88 minimum) as ADR 0012.

**Blocked by:** 01

**Status:** resolved

- [x] A test outside the crate (an integration test or the fixture crate) calls `rok_sqlc::generate` with a `Config` and gets the same files the CLI writes for the same inputs.
- [x] The returned report lists written, unchanged, and refused files, plus table and query failures.
- [x] Internal modules no longer appear as part of the documented public API (they are `pub(crate)` or `#[doc(hidden)]`); existing tests keep compiling, through test-only access where needed.
- [x] All existing CLI, corpus, and fixture tests pass unchanged in behavior.
- [x] ADR 0012 is written.

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
