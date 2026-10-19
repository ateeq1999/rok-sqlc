# 01: Unique indexes on `lower(col)`

**What to build:** rok-sqlc ignores expression indexes, so `CREATE UNIQUE INDEX users_email_key ON users (lower(email)) WHERE deleted_at IS NULL` makes no unique key. Recognise a unique index whose single expression is `lower(<column>)` (any casing of `lower`, the column optionally quoted). Treat it as a case-insensitive unique key on that column:
- `UniqueKey` gains a flag, such as `case_insensitive`;
- the partial-index rules that already apply keep applying (a `deleted_at IS NULL` predicate counts under the soft-delete convention);
- the data layer's lookup for it (`get_by_email`) compares `lower(col) = lower($1)`;
- the API lookup route works the same way.

Other expression indexes stay ignored, as today.

This changes output only for schemas that have such an index: they now gain the lookup. Check the module corpus and fixtures for any, and record the effect in the CHANGELOG.

**Blocked by:** None

**Status:** ready-for-agent

- [ ] Parser tests (variants, plus non-`lower` expressions still ignored) and data-layer tests: a case-insensitive lookup against PostgreSQL.
- [ ] The template (`users_email_key`) now has `email` as a unique key; its contract still passes. Its skipped `lookups` keep the route out.
