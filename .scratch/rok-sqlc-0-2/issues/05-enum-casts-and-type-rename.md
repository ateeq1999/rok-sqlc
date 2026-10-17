# 05: Enum casts in query files and `ALTER TYPE … RENAME TO`

**What to build:**
- **Enum casts:** in query files, a cast to an enum type infers the enum. `$1::product_status` and `$1::app.ticket_state` infer `types::X`, and the array form `$1::product_status[]` infers `&[types::X]`.
- **Type rename:** `ALTER TYPE x RENAME TO y` renames the enum. The Rust name follows the new name and `type_name` updates. Columns that use the enum keep working.

`ALTER TYPE … RENAME VALUE` already works.

**Blocked by:** None

**Status:** resolved

- [x] A query-file function with an enum cast parameter compiles and runs against PostgreSQL.
- [x] A migration that renames an enum generates the new name, and the fixture decodes it against PostgreSQL.

## Comments

Resolved on integration branch `rok-sqlc-0-2`. Review fixes in 760d008..e2255b5, large-file refactor up to 6fa10e3, release prep in the 0.2.0 commit. See CHANGELOG.md [0.2.0] and the ADRs.
