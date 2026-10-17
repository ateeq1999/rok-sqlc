# 02: The tenant column is not a filter field or sort key

**What to build:** On tables with the tenant convention:
- `{Table}Filter` has no field for the tenant column, including `_is_null`.
- `{Table}SortKey` has no variant for it.
- Relationship filters are unchanged.

This is a breaking change to the generated API. Record it under Breaking in CHANGELOG and in ADR 0010.

**Blocked by:** None

**Status:** ready-for-agent

- [ ] The generated filter and sort key for a tenant table leave out the tenant column. Tables without the convention generate unchanged (CLI seam).
- [ ] fixture-app and the module harness stay green, including the lint gate and PostgreSQL.
