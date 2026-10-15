# 17: Soft delete convention

**What to build:** For tables with the configured soft-delete column (which must be a nullable `TIMESTAMP` or `TIMESTAMPTZ`, otherwise a diagnostic):
- `delete` sets it to `now()` where it is null and returns whether a row changed. `restore` clears it. `delete_permanently` removes the row.
- Every generated read (`get_by_id`, unique-key lookups, `exists_by_id`, `list`, `list_page`, `count`, `search`, `search_after`) skips soft-deleted rows, and the search filter gains `include_deleted: bool`.
- `update` on a soft-deleted row returns not-found.
- The column is excluded from inputs and patches.
- Query files are never rewritten.

**Blocked by:** 16, 15

**Status:** ready-for-agent

- [ ] Delete, then every read misses the row; restore, then they find it again; permanent delete removes it (PostgreSQL seam).
- [ ] `delete` on an already-deleted row returns `false`, and `update` on it returns not-found.
- [ ] `include_deleted: true` returns deleted rows through `search`.
- [ ] A soft-delete-scoped partial unique index allows a deleted and a live row with the same key, and `get_by_<key>` returns the live one.
- [ ] A wrongly typed soft-delete column fails generation with a diagnostic.
