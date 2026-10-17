# 23: Many-to-many: inference, loaders, `attach`/`detach`/`sync`

**What to build:** A **join table** is a table whose two non-tenant foreign keys, to different tables, together form a unique key. Payload columns are allowed. It gives a many-to-many relationship on both sides (roles ↔ permissions through `role_permissions`). The relationship gets loaders like has-many: a map from typed id to a list of related rows, loaded through the join table. The join table's resource gets `attach(a, b)` (idempotent), `detach(a, b)`, and `sync(a, &[b])`, which replaces the set inside the caller's transaction. On a soft-delete join table, `detach` soft-deletes and `attach` restores. Tenant scoping applies.

**Blocked by:** 20

**Status:** resolved

- [x] Attaching, detaching, and syncing permissions on a role yields the expected sets through the loader (PostgreSQL seam).
- [x] `attach` twice keeps one row; `sync` with an empty slice removes all.
- [x] On a soft-delete join table, detach-then-attach restores the original row rather than inserting a duplicate.
- [x] A table with two foreign keys but no unique pair (like `user_roles`) is not treated as a join table.

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
