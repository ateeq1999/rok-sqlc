# 03: Index, sequence, view and constraint renames

**What to build:** Track renames for default constraint names:
- `ALTER INDEX [IF EXISTS] i RENAME TO j`, `ALTER SEQUENCE …`, `ALTER VIEW …` and `ALTER MATERIALIZED VIEW … RENAME TO`: the old name is free and the new one taken. The parser reads only the first form without `IF EXISTS`, so recognise the rest from tokens.
- Renaming the index behind a primary key or unique constraint renames the constraint too, as in PostgreSQL. The renamed constraint is the name `ON CONFLICT ON CONSTRAINT` accepts.
- `ALTER TABLE t RENAME CONSTRAINT a TO b` renames a primary key or unique constraint. Other constraint names aren't tracked, so for them this changes nothing.

**Blocked by:** 02

**Status:** resolved

- [x] A default constraint name created after a rename uses the freed name.
- [x] `ON CONFLICT ON CONSTRAINT` accepts the renamed constraint and rejects the old name.

## Comments

Done on branch `rok-sqlc-0-2-1`. See CHANGELOG.md [0.2.1].
