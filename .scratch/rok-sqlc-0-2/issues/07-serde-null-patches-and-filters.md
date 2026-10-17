# 07: serde can clear a column through a patch; filters get serde

**What to build:** Two serde changes, both behind `derive = ["serde"]`.

- **Patches can clear a column.** Patch fields have type `Option<Option<T>>`.
  - On deserialize, an absent field means leave the column alone, and `null` means set it to NULL.
  - On serialize, absent fields are skipped.
  - Implement this with a small generated helper in `db/` rather than a new dependency.
- **Search types get serde.** `{Table}Filter`, the condition types in `db::search` and the sort keys all derive serde. That lets API layers accept filters as JSON.

Update ADR 0013.

**Blocked by:** 02

**Status:** ready-for-agent

- [ ] Against PostgreSQL, deserializing `{}` leaves a column untouched, and `{"col": null}` sets it to NULL.
- [ ] A filter round-trips through JSON and returns the same rows.
