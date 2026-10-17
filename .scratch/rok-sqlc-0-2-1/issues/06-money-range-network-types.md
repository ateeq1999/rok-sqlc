# 06: `MONEY`, range and network types

**What to build:** Map more PostgreSQL types:

| PostgreSQL | Rust | SQLx feature |
|---|---|---|
| `MONEY` | `sqlx::postgres::types::PgMoney` | — |
| `INT4RANGE` | `sqlx::postgres::types::PgRange<i32>` | — |
| `INT8RANGE` | `sqlx::postgres::types::PgRange<i64>` | — |
| `NUMRANGE` | `sqlx::postgres::types::PgRange<rust_decimal::Decimal>` | `rust_decimal` |
| `DATERANGE` | `sqlx::postgres::types::PgRange<chrono::NaiveDate>` | `chrono` |
| `TSRANGE` | `sqlx::postgres::types::PgRange<chrono::NaiveDateTime>` | `chrono` |
| `TSTZRANGE` | `sqlx::postgres::types::PgRange<chrono::DateTime<chrono::Utc>>` | `chrono` |
| `INET`, `CIDR` | `ipnetwork::IpNetwork` | `ipnetwork` |
| `MACADDR` | `mac_address::MacAddress` | `mac_address` |

Cover columns, query-file casts, and the `db/mod.rs` dependency hints. All of them get equality filters.

`PgMoney` and `PgRange` implement no serde traits, so under `derive = ["serde"]` they fail like `INTERVAL`. `ipnetwork` and `mac_address` have serde features, which the hints name.

**Blocked by:** None

**Status:** resolved

- [x] A fixture table with these types round-trips create/get/search against PostgreSQL.
- [x] Query-file casts (`$1::inet`) infer the mapped types.

## Comments

Done on branch `rok-sqlc-0-2-1`. See CHANGELOG.md [0.2.1].
