# 27: Generated code targets SQLx 0.9

**What to build:** Generated code compiles and runs against SQLx 0.9 (0.9.0, stable) instead of 0.8. The research notes on what changed are at `C:/Users/atega/AppData/Local/Temp/claude/d--DEV-open-source-PROJECT-ROK-rok-sqlc/636427fe-329e-4eb3-974a-b34c14e48ce7/scratchpad/notes/sqlx-0.9.md`. The main changes:
- **Generator:** `QueryBuilder` no longer takes a lifetime parameter, so the generated search code and its `Encode` bounds must change. The dependency hint printed for users becomes `sqlx = "0.9"` with the same feature names.
- **Fixture crates:** `tests/fixture-app` and `tests/modules-app` move to sqlx 0.9 (manifests and lockfiles). Their hand-written SQL calls must pass `&'static str` or `AssertSqlSafe(...)`, because 0.9's `query*()`/`raw_sql()` accept only those.
- **Rust floors:** generated code needs Rust 1.94 (SQLx 0.9's MSRV), while the tool itself keeps `rust-version = "1.88"`.
  - The fixture crates declare `rust-version = "1.94"`.
  - CI keeps testing the tool on 1.88 and stable. Only suites that compile generated code (fixture compile, module fixtures, PostgreSQL) need ≥ 1.94. The 1.88 job either skips them explicitly or a 1.94 job runs them. Either way, both floors stay tested.
  - `tests/release_metadata.rs` is updated to match.
- **Docs:** ADR 0012 (the library contract says "SQLx 0.8 only") and the CHANGELOG's Unreleased section record the move. README's dependency snippet changes to 0.9. The full README rewrite is still ticket 26.

**Blocked by:** 09, 12, 13

**Status:** resolved

- [x] The fixture crate and the module fixture crate build against sqlx 0.9 under `#![deny(warnings)]`, and their PostgreSQL suites pass. That includes enums with schema-qualified type names and arrays of enums, typed ids, key structs, and keyset search (PostgreSQL seam).
- [x] No generated file mentions a `QueryBuilder` lifetime, and the dependency hint names sqlx 0.9.
- [x] `cargo +1.88 test` for the tool passes, with any skips of generated-code suites explicit. A ≥ 1.94 toolchain runs the full suite including the fixtures.
- [x] ADR 0012, CHANGELOG and the README snippet say SQLx 0.9 and give the 1.94 floor for generated code.

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
