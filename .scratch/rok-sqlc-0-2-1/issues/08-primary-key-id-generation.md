# 08: Primary key ids made by the generated code

**What to build:** A `[conventions]` key, `primary_key_ids`, choosing how generated code makes new primary keys: `"uuid_v4"`, `"uuid_v7"`, `"ulid"`, or `"none"`.

It applies to tables whose primary key is one `UUID` column, or with `"ulid"` also one text column:
- the typed id gets `generate()`;
- the create input takes the key as `Option<Id>`;
- `create` binds `input.id.unwrap_or_else(Id::generate)`, so `None` inserts a new id, even over a database default.

Composite keys, other key types, and generated or managed key columns are left alone. `db/mod.rs` names the `uuid` features (`v4`, `v7`) or the `ulid` crate (with its `uuid` feature for `UUID` keys).

The maintainer asked to use rok-utils (`uuid_v4`, `uuid_v7`, `ulid`) for this. It isn't used:
- it needs Rust 1.92, above the tool's 1.88;
- its helpers return `String`, while keys are `uuid::Uuid`;
- its `ulid()` isn't a ULID: 16 hex characters of milliseconds and 10 base-36 characters, where a ULID is 26 Crockford base-32 characters. `is_ulid` accepts any 26 alphanumerics.

The generated code calls `uuid` and `ulid` directly, like every other type it uses.

**Blocked by:** None

**Status:** resolved

- [x] `uuid_v7` keys are made by `create` against PostgreSQL, over a `gen_random_uuid()` default too, and a given id is kept.
- [x] Each strategy renders its constructor, input type, bind, and dependency hint; other key types are untouched.
- [x] An unknown strategy fails the run, listing the valid ones.

## Comments

Done on branch `rok-sqlc-0-2-1`. See CHANGELOG.md [0.2.1] and ADR 0010.
