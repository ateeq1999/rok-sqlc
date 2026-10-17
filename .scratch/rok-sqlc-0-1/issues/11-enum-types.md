# 11: Enum types become a shared `types` module

**What to build:** `CREATE TYPE … AS ENUM (…)` generates a Rust enum in a shared generated `types` module. The enum derives `sqlx::Type` with the database type name, including its schema when qualified, and every variant keeps its exact database label through a per-variant rename. `ALTER TYPE … ADD VALUE` adds a variant. Columns of an enum type use the enum in rows, inputs, patches, filters (equality conditions), sort keys, and query-file parameter and result types. Arrays of enums are supported. `types` becomes a reserved module name.

**Blocked by:** 10

**Status:** resolved

- [x] An enum with mixed-case and upper-case labels round-trips through create, get, update, and search against PostgreSQL (PostgreSQL seam).
- [x] An array-of-enum column round-trips.
- [x] `ALTER TYPE … ADD VALUE` makes the new value usable.
- [x] A query file comparing an enum column to a placeholder infers the enum as the parameter type.
- [x] The corpus `hr` fixture's `employment_type` enum generates and `employees.employment_type` uses it (update the corpus expectations); `employees` stays rejected for `tenure INTERVAL`, see Out of Scope.

## Out of Scope

- `INTERVAL` and other unmapped column types: a table with one stays rejected (README, Limitations).

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
