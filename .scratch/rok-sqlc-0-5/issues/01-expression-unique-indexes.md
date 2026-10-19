# 01: Unique indexes on `lower(col)`

**What to build:** rok-sqlc ignores expression indexes, so `CREATE UNIQUE INDEX users_email_key ON users (lower(email)) WHERE deleted_at IS NULL` makes no unique key. Recognise a unique index whose single expression is `lower(<column>)` (any casing of `lower`, the column optionally quoted). Treat it as a case-insensitive unique key on that column:
- `UniqueKey` gains a flag, such as `case_insensitive`;
- the partial-index rules that already apply keep applying (a `deleted_at IS NULL` predicate counts under the soft-delete convention);
- the data layer's lookup for it (`get_by_email`) compares `lower(col) = lower($1)`;
- the API lookup route works the same way.

Other expression indexes stay ignored, as today.

This changes output only for schemas that have such an index: they now gain the lookup. Check the module corpus and fixtures for any, and record the effect in the CHANGELOG.

**Blocked by:** None

**Status:** resolved

- [x] Parser tests (variants, plus non-`lower` expressions still ignored) and data-layer tests: a case-insensitive lookup against PostgreSQL.
- [x] The template (`users_email_key`) now has `email` as a unique key; its contract still passes. Its skipped `lookups` keep the route out.

## Comments

Resolved on branch `rok-sqlc-0-5-01`.

**What ticket 02 needs to know:**
- **Model:** `UniqueKey { columns, predicate, case_insensitive: bool }` (`src/schema/types.rs`). A case-insensitive key always has exactly one column. It comes only from a unique index whose only entry is `lower(<col>)`. Composite indexes that include a `lower()` entry, such as the module corpus's `(organization_id, lower(name))`, are still ignored.
- **Which keys count:**
  - `Table::lookup_unique_keys(soft_delete)` returns the counting keys with the flag kept: primary key first, partial indexes only on `<soft-delete> IS NULL`, and duplicate column sets dropped (the first declared wins).
  - `Table::unique_key_columns(soft_delete)` leaves case-insensitive keys out, because `=` doesn't respect them. `ON CONFLICT` checking and has-one inference use it.
  - `table.unique_keys` is the raw list. `paths.rs` still uses it for 409 documentation.
- **How to query:** compare `lower(<col>) = lower($n)`. `generator::sql::live_lowered_predicate(table, column)` builds the tenant condition, then `lower(col) = lower($first)`, then the live condition. For the pre-check, add `AND <pk> <> $k` on update.
- **Lookups:** `read::Lookup` and `generator::read::unique_lookups` carry `case_insensitive`. The template's `users` now has `read::get_by_email`, case-insensitive and live-only. Its `lookups` stay skipped, and its update documents 409.
