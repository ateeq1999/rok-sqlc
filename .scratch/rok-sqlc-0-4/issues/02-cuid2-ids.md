# 02: The `cuid2` primary key strategy

**What to build:** `primary_key_ids = "cuid2"` (`IdGeneration::Cuid2`) applies to tables keyed by one text column. The typed id gets `generate()` returning `Self(cuid::cuid2())`, and `create` fills a missing key the same way the other strategies do. Check the `cuid` crate's 2.x API in the cargo registry; the dependency hint is `cuid = "2"`. It doesn't apply to `UUID` keys: such a table is left alone, as `ulid` is on keys of other types. Follow how `uuid_v4`/`uuid_v7`/`ulid` flow through `src/config.rs`, `src/config_file.rs`, `src/generator/{conventions,keys,db_mod}.rs`, and `tests/conventions/ids.rs`. Update the README conventions table and ADR 0010's primary key ids paragraph.

**Blocked by:** None

**Status:** resolved

- [x] Convention tests cover the generated `generate()`, input and bind, the hint, and a UUID key left alone.
- [x] The fixture app creates a row with a generated cuid2 key against PostgreSQL.

## Comments

Implemented on branch `rok-sqlc-0-4-02`. The fixture app gets a fourth generated module, `src/cuid_db/` (from `cuid_schema.sql`, with a defaulted text key and a `UUID` key), so `plain_db` keeps its `uuid_v7` keys. `cuid::cuid2()` comes from cuid 2.0.4 (`pub use cuid2::cuid as cuid2`).
