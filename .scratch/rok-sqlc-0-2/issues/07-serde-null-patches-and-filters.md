# 07: serde can clear a column through a patch; filters get serde

**What to build:** Two serde changes, both behind `derive = ["serde"]`.

- **Patches can clear a column.** Patch fields have type `Option<Option<T>>`.
  - On deserialize, an absent field means leave the column alone, and `null` means set it to NULL.
  - On serialize, absent fields are skipped.
  - Implement this with a small generated helper in `db/` rather than a new dependency.
- **Search types get serde.** `{Table}Filter`, the condition types in `db::search` and the sort keys all derive serde. That lets API layers accept filters as JSON.

Update ADR 0013.

**Blocked by:** 02

**Status:** resolved

- [x] Against PostgreSQL, deserializing `{}` leaves a column untouched, and `{"col": null}` sets it to NULL.
- [x] A filter round-trips through JSON and returns the same rows.

## Comments

Resolved on integration branch `rok-sqlc-0-2`. Review fixes in 760d008..e2255b5, large-file refactor up to 6fa10e3, release prep in the 0.2.0 commit. See CHANGELOG.md [0.2.0] and the ADRs.
