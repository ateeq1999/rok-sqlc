# 25: `WITH` (CTE) support in query files

**What to build:** Query files accept `WITH name AS (SELECT …) SELECT …`, including several CTEs and CTEs that reference earlier ones. Each CTE's result columns are inferred like a `SELECT`, with explicit CTE column lists honoured, and become a scope the following CTEs and the main statement can reference by name or alias. Placeholders inside CTEs are typed by the same rules as elsewhere. `WITH RECURSIVE` and data-modifying CTEs (`WITH … AS (INSERT/UPDATE/DELETE …)`) are rejected with a diagnostic for now.

**Blocked by:** None (can start immediately)

**Status:** resolved

- [x] A query using a CTE to pre-aggregate stock per category, then joining it to categories, generates a typed row struct and returns correct rows against PostgreSQL (PostgreSQL seam).
- [x] A placeholder inside the CTE is inferred from the column it is compared with.
- [x] A recursive CTE and a data-modifying CTE fail with `path:line` diagnostics that say they are unsupported.

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
