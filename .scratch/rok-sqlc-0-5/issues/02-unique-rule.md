# 02: The `unique` pre-check

**What to build:** the spec's `unique` decision. For every single-column unique key of a feature's table (including ticket 01's case-insensitive ones) and every composite unique key, the generated `create_in`/`update_in` defaults run a pre-check before writing, on the transaction's connection:
- **What counts:** live rows only (soft-deleted rows don't count), within the tenant, excluding the row being updated.
- **When:** only for columns present in the input (or patch).
- **Answer:** a taken value gives `422 validation_failed`, with `details` keyed by column, code `unique`, message `"<column> is already taken"`. A composite key reports against its last column, as `"<col a> and <col b> are already taken together"`.
- **Ordering:** every unique and `exists` failure (ticket 03) is collected, so one 422 lists them all.
- **Fallback:** a race that still hits the constraint keeps answering `409 conflict`.

**Where it lives:** add the check's query to the data layer (`db/<table>/listing.rs` or a new `checks.rs`, keeping rule 2 and `check-arch` clean). Add `AppError` support for the field-level 422, reusing `Validation { details }`.

**OpenAPI:** create and update already document 422, so nothing new is needed.

**Template and contract:** `users.email` (now case-insensitive unique) gets the check. The contract test should still pass, since statuses beyond the target's are allowed.

**Docs:** wire-standard.md (code `unique`), configuring.md (it's automatic, and how to opt out by skipping or overriding `create_in`), CHANGELOG (breaking: 409 becomes 422 for duplicates).

**Blocked by:** 01, 03

**Status:** resolved

- [x] `tests/api-app` against PostgreSQL covers:
  - a duplicate on create and on update;
  - a case-insensitive duplicate;
  - a soft-deleted duplicate allowed;
  - another tenant's value allowed;
  - a composite duplicate;
  - updating a row to its own value allowed;
  - several failures in one 422.
- [x] The race fallback stays 409 (documented and unit-tested at the error-mapping level).

## Comments

Resolved on branch `rok-sqlc-0-5-02`.

- **Generator:** `src/generator/api/unique.rs`. `unique::checks(feature)` picks the keys from `Table::lookup_unique_keys` (primary key included), less the tenant column. A key is checked on create or update when that body holds every one of its columns. The data layer's `listing.rs` gets `<cols>_taken(executor, [tenant], values.., except: Option<Key>)`, which excludes the row with `IS DISTINCT FROM`. `prechecks::push_checks` renders the checks after the `exists` ones, before `into_result()?`. `FieldFailures` gains `taken(field)` and `taken_together(&[fields])`.
- **Fixture:** `tests/api-app/migrations/0007_handles.sql` (`handles`: tenant-scoped, soft-deleted, `lower(email)`, `(tenant_id, nickname)`, `(tenant_id, region, seat)`, all partial on `deleted_at IS NULL`). The tests are in `tests/api-app/tests/unique.rs`, including the race fallback (`AppError::from` a real unique violation is a `409`). `notes.rs` now expects 422 for a duplicate slug.
- **Deviations:**
  - Soft-deleted rows are left out only for a key that is partial on the soft-delete column. A full constraint counts them too, as the database does, so its duplicate is a 422 rather than a 409.
  - Not checked; these keep the 409:
    - a key including the owner column;
    - a patch holding only some of a composite key's columns;
    - a column bound neither by value nor by borrow.
  - The template's `users.email` gets no check. `create` is skipped and `email` is immutable, so neither body holds it. Only the template's `shared/validation.rs` changes.
