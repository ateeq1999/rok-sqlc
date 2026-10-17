# 02: The tenant column is not a filter field or sort key

**What to build:** On tables with the tenant convention:
- `{Table}Filter` has no field for the tenant column, including `_is_null`.
- `{Table}SortKey` has no variant for it.
- Relationship filters are unchanged.

This is a breaking change to the generated API. Record it under Breaking in CHANGELOG and in ADR 0010.

**Blocked by:** None

**Status:** resolved

- [x] The generated filter and sort key for a tenant table leave out the tenant column. Tables without the convention generate unchanged (CLI seam).
- [x] fixture-app and the module harness stay green, including the lint gate and PostgreSQL.

## Comments

Resolved on integration branch `rok-sqlc-0-2`. Review fixes in 760d008..e2255b5, large-file refactor up to 6fa10e3, release prep in the 0.2.0 commit. See CHANGELOG.md [0.2.0] and the ADRs.
