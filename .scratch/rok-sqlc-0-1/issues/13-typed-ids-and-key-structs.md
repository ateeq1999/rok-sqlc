# 13: Typed ids, key structs, and exact cross-schema foreign-key resolution

**What to build:** Every table with a single-column primary key gets a **typed id** (for example `ProductId`). It is a transparent SQLx type that is `Copy`, `Eq`, `Hash`, `Ord`, `Debug`, and `Display`, converts to and from the inner value, and sits in the resource's models. The row's key field, every foreign-key field pointing at that table, and every function taking or returning the key use the typed id. Composite primary keys get a key struct (`OrderItemKey { order_id, product_id }`), used by `get_by_id`-style lookups and `search_after` cursors instead of separate parameters and tuples. Foreign keys record the referenced schema, so a reference to `core.users` resolves to exactly that table. A foreign key to a table that isn't generated keeps the raw type.

**Blocked by:** 10

**Status:** resolved

- [x] Fixture code passing a `CategoryId` where a `ProductId` is expected fails to compile (verify with a compile-fail check or by documented negative test), and the positive paths compile and run against PostgreSQL (PostgreSQL seam).
- [x] A foreign key field (`products.category_id`) has type `CategoryId`, and a value read from a category can be used directly to filter products.
- [x] Composite-key lookups and keyset pagination take and return the key struct.
- [x] Two same-named tables in different schemas are referenced correctly by foreign keys.
- [x] Existing PostgreSQL scenarios are updated to the new signatures and still pass; the module fixture harness passes.

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
