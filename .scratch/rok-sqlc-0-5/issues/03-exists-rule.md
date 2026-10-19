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

**Status:** resolved

- [x] `tests/api-app` against PostgreSQL covers:
  - a missing FK on create and on update;
  - another owner's row treated as missing;
  - another tenant's row treated as missing;
  - a soft-deleted referenced row treated as missing;
  - a `null` nullable FK allowed.
- [x] Lint gate, `check-arch`, and the template's contract all pass.

## Comments

Resolved on branch `rok-sqlc-0-5-03`.

**The collector (for ticket 02):** `FieldFailures` in the regenerated `shared/validation.rs` (template in `src/generator/api/shared_validation.rs`, split out of `shared.rs`). It wraps `validator::ValidationErrors`, so `details` has the body rules' shape:
- `add(&mut self, field: &'static str, code: &'static str, message: String)`: any failure; 02 can call it with `"unique"` and `"<column> is already taken"` (or add a `taken(field)` helper beside `missing`).
- `missing(field, singular)`: the `exists` failure.
- `is_empty()`, and `into_result(self) -> Result<(), AppError>` (`AppError::from(ValidationErrors)`, message "the request failed validation").

**The generator side:** `src/generator/api/prechecks.rs`. `Reference` (a body's FK, with `on_create`/`on_update` and `owned`) is computed per feature in `api::features()` into `Feature::references`. `prechecks::import(uses, feature, endpoint)` adds the imports and `prechecks::push_checks(output, feature, endpoint)` renders the block in `create_in`/`update_in` (after the owner lock, before the write): `let mut failures = FieldFailures::default();`, one check per reference, then `failures.into_result()?;`. For 02: add your checks to `push_checks` (and `import`) between the reference checks and `into_result`, so both kinds land in one 422. `dto::create_columns`/`update_columns` are now `pub(super)` for this.

**Queries:** a reference to a non-owned table uses the data layer's `read::exists_by_id` (already tenant-scoped and live-only); to an owned feature's table, from an owned feature, the new `listing::exists_owned(executor, [tenant], id, owner)`. From a feature without an owner, a reference to an owned table uses `exists_by_id`, because the caller isn't known there.

**Not checked (the constraint's 409 still applies):** a tenant-scoped target referenced from a feature without the tenant column, and an FK that isn't the target's whole single-column key, has a different type, or isn't `Copy`/borrowed-text.

**Fixture:** `tests/api-app/migrations/0005_comments.sql` adds `comments` (tenant-scoped, references `documents`), which covers the tenant and soft-delete cases. The tests are in `tests/api-app/tests/references.rs`. The `members` FK assertion in `shapes.rs` now expects a 422. The template's features have no FK in a body, so only its `shared/validation.rs` changes.
