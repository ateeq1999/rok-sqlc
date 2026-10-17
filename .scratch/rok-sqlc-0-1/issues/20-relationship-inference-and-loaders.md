# 20: Relationship inference and loaders (belongs-to, has-many, has-one)

**What to build:** Relationships are inferred from foreign keys:
- **Belongs-to** for each foreign key except the tenant column's, named from the column without `_id`. Foreign keys to unique non-key columns count.
- **Has-many** as the inverse, named after the child module, or `{child}_by_{relation}` when a child has several foreign keys to the same parent.
- **Has-one** when the foreign key column is a unique key.
- **Self-references** through `parent_id` give `parent` and `children`.

Each resource gets a relationships module of **loaders**. Each loader takes a slice of already-fetched rows and loads one relationship for all of them in a single query using `= ANY(…)`. To-one loaders return a map from the related typed id to the related row; to-many loaders return a map from the parent typed id to a list. Loaders respect soft delete and tenant scoping. Name collisions between relationships fail generation with a diagnostic. Record the relationship design (`with` and `join`) as ADR 0014.

**Blocked by:** 15, 17, 18

**Status:** resolved

- [x] Loading `category` for 50 products issues one query and maps each product's `category_id` to its category (PostgreSQL seam; verify the query count through the database's statement statistics or a counting executor in the test).
- [x] Loading `comments` for posts returns each post's comments, including empty lists, and excludes soft-deleted comments and other tenants' rows.
- [x] Two foreign keys from `invoices` to `users` yield distinct, predictable relationship names.
- [x] A self-referencing table loads `parent` and `children`.
- [x] The module fixture harness compiles with relationships for the whole corpus.
- [x] ADR 0014 is written.

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
