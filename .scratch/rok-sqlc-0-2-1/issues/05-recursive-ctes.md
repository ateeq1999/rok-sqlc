# 05: `WITH RECURSIVE` in query files

**What to build:** Analyze a recursive CTE whose body is `anchor UNION [ALL] recursive-term`:
- analyze the anchor first; its outputs (and the CTE's column list) give the CTE's columns;
- then analyze the recursive term with the CTE in scope. It must return as many columns. A column whose type is known must have the anchor's type, and a nullable column makes the CTE's column nullable.

A CTE in a `WITH RECURSIVE` list whose body isn't a `UNION` is analyzed as before. `UNION` elsewhere and data-modifying CTEs stay unsupported.

**Blocked by:** None

**Status:** resolved

- [x] A tree walk over a self-referencing table generates and runs against PostgreSQL.
- [x] A recursive term with a mismatched column count or type fails with a clear error.

## Comments

Done on branch `rok-sqlc-0-2-1`. See CHANGELOG.md [0.2.1].
