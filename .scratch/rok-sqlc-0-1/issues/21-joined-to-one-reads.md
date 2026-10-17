# 21: Joined reads for to-one relationships

**What to build:** For each to-one relationship (belongs-to and has-one), a single-query joined read, `get_with_<relation>(…key…) -> Option<{Row}With{Relation}>`. The generated struct holds both rows; when the foreign key is nullable, the related row is optional. The join respects soft delete (a deleted parent reads as absent, it does not hide the child) and tenant scoping on both sides.

**Blocked by:** 20

**Status:** resolved

- [x] `get_with_category` returns the product with its category in one round trip (PostgreSQL seam).
- [x] A nullable foreign key with no parent yields the row with `None` as the related row.
- [x] A soft-deleted parent yields the child with no related row; a soft-deleted child yields `None`.
- [x] Result columns from both tables are decoded unambiguously even when the tables share column names.

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
