# 15: Unique keys: `get_by_<cols>` lookups and `ON CONFLICT` checks

**What to build:** **Unique keys** are the primary key, unique constraints (inline, table-level, or via `ALTER TABLE ADD CONSTRAINT`), and unique indexes on plain columns. A partial unique index counts only when its condition is exactly `<soft-delete column> IS NULL`; other partial indexes and expression indexes are skipped. Every unique key other than the primary key generates `get_by_<col>[_and_<col>…]` returning `Option<Row>`, using typed ids and borrowed text. In query files, an `ON CONFLICT (cols)` target must match a unique key; otherwise generation fails and names the unique keys that do exist.

**Blocked by:** 13, 10, 27

**Status:** resolved

- [x] `get_by_sku` and a composite `get_by_organization_id_and_code` return the right row, or `None`, against PostgreSQL (PostgreSQL seam).
- [x] An expression index (`lower(name)`) and a non-soft-delete partial index generate no lookup.
- [x] A query-file upsert whose `ON CONFLICT` target isn't a unique key fails generation with a `path:line` diagnostic listing the available keys (CLI seam).
- [x] The inventory fixture's `UpsertProduct` is now rejected unless `sku` is unique; corpus expectations are updated.

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
