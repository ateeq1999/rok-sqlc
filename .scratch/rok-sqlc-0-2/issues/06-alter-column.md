# 06: `ALTER TABLE … ALTER COLUMN`

**What to build:** Apply the common `ALTER COLUMN` forms instead of failing the table:
- `ALTER COLUMN … TYPE …`, with or without `USING`;
- `SET NOT NULL` and `DROP NOT NULL`;
- `SET DEFAULT` and `DROP DEFAULT`;
- `ADD IDENTITY` and `DROP IDENTITY`, if simple.

The column ends up in its final shape. Keys and unique keys follow a type change.

Any other `ALTER COLUMN` form still fails with `path:line`.

This interacts with ticket 03: dropping a default can make a managed column fail.

**Blocked by:** 03

**Status:** ready-for-agent

- [ ] A migration that widens a type, adds NOT NULL and drops a default produces a model whose generated code round-trips against PostgreSQL with the final shape.
- [ ] Unsupported ALTER COLUMN forms still fail with path:line.
