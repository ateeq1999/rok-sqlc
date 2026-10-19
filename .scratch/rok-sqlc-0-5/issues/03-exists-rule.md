# 03: The `exists` pre-check

**What to build:** the spec's `exists` decision. For every foreign-key field present in a create input or update patch, the generated `create_in`/`update_in` defaults check, on the transaction's connection, that the referenced row exists:
- **What counts as visible:** respect the referenced table's owner scoping (the caller's own rows), tenant scoping (same tenant), and soft delete (live rows only), when the referenced table is a generated feature with those conventions. Otherwise use a plain existence check.
- **Answer:** a missing row gives `422 validation_failed`, with `details` keyed by column, code `exists`, message `"<column> does not refer to an existing <singular>"` (`<singular>` in the path-description style from 0.4: "category").
- **Nullable FKs:** `null` (and absent) values are skipped.
- **Collecting:** structure the collector so ticket 02's `unique` failures join the same 422. Build the shared collector here (a small type in the generated service or `shared/`), and have 02 reuse it.
- **Fallback:** the FK constraint stays, answering `409` on a race.

**Where it lives:** add the queries to the data layer, keeping `check-arch` clean. Reading another table is allowed; only writes go through ports.

**Docs:** wire-standard.md (code `exists`), CHANGELOG (breaking: 409 `reference_missing`-style conflicts become 422 for missing references).

**Blocked by:** None

**Status:** ready-for-agent

- [ ] `tests/api-app` against PostgreSQL covers:
  - a missing FK on create and on update;
  - another owner's row treated as missing;
  - another tenant's row treated as missing;
  - a soft-deleted referenced row treated as missing;
  - a `null` nullable FK allowed.
- [ ] Lint gate, `check-arch`, and the template's contract all pass.
