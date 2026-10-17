# 04: `INTERVAL`, `BYTEA` and `TIME WITH TIME ZONE`

**What to build:** Map three more PostgreSQL types:

| PostgreSQL | Rust |
|---|---|
| `INTERVAL` | `sqlx::postgres::types::PgInterval` |
| `BYTEA` | `Vec<u8>` (bound as `&[u8]` where parameters are borrowed) |
| `TIMETZ` | `sqlx::postgres::types::PgTimeTz` |

Cover columns, arrays where SQLx supports them, query-file casts (`$1::interval`) and result columns.

Filters:
- `bytea` gets equality.
- `interval` gets ordered filters if SQLx implements `PartialOrd` for it; otherwise equality.

Check how these types behave under `derive = ["serde"]`. If they don't implement serde, either document what such tables do or make them fail clearly.

Update the README's type table and limitations.

**Blocked by:** None

**Status:** resolved

- [x] A fixture table with all three types round-trips create/get/update/search against PostgreSQL.
- [x] The corpus `hr` legacy `employees` table (`tenure INTERVAL`) now generates. Update `EXPECTED`.
- [x] Query-file casts to these types infer the mapped types.

## Comments

Resolved on integration branch `rok-sqlc-0-2`. Review fixes in 760d008..e2255b5, large-file refactor up to 6fa10e3, release prep in the 0.2.0 commit. See CHANGELOG.md [0.2.0] and the ADRs.
