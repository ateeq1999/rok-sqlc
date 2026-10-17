# 18: Tenant scoping and `db::session`

**What to build:** For tables with the configured **tenant column**, every generated function takes the tenant id immediately after the executor and adds `tenant_column = $n` to every statement (reads, writes, search, `search_after`, unique-key lookups). The tenant column is removed from create inputs (it is set from the parameter) and from update patches (it can never change). When the tenant column is part of the primary key, the tenant parameter replaces that key column. The tenant column's foreign key is not exposed as a relationship (see ticket 20). With `tenant_setting` and/or `user_setting` configured, a generated `session` module provides `set_tenant(&mut tx, id)` and `set_user(&mut tx, id)`, which run a transaction-local `set_config`. `session` becomes a reserved module name.

**Blocked by:** 16

**Status:** resolved

- [x] Rows created under tenant A are invisible to every read, update, and delete made with tenant B (PostgreSQL seam).
- [x] A table with a row-level security policy reading the tenant setting works when `session::set_tenant` is called in the transaction, and returns nothing when it isn't.
- [x] A composite primary key that includes the tenant column takes the tenant only once.
- [x] Without the convention, generated signatures are unchanged.

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
