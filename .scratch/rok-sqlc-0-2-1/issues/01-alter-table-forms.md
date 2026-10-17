# 01: `ALTER TABLE` forms that don't change the model

**What to build:** Stop failing tables over `ALTER TABLE` clauses whose effect on the model is known.

The parser can't read these. Recognise them from tokens when a statement fails to parse:
- `ALTER COLUMN c SET STATISTICS n`, `SET STORAGE …`, `SET COMPRESSION …`, `SET (…)`, `RESET (…)`: no change.
- `ALTER COLUMN c SET GENERATED { ALWAYS | BY DEFAULT }`, `RESTART [WITH n]`, and `SET <sequence option>`: the column stays an identity column, so no change.
- `ALTER COLUMN c DROP IDENTITY [IF EXISTS]` and `DROP EXPRESSION [IF EXISTS]`: the database no longer owns the value, so this behaves like `DROP DEFAULT` (no default, not generated; a managed column loses its source here).
- `ALTER COLUMN c [SET DATA] TYPE t COLLATE x [USING …]`: the collation doesn't change the Rust type, so read it without `COLLATE x`.
- Table level: `RESET (…)`, `CLUSTER ON`, `SET WITHOUT CLUSTER`, `SET TABLESPACE`, `SET LOGGED`, `SET UNLOGGED`, `[NO] FORCE ROW LEVEL SECURITY`, `ALTER CONSTRAINT`: no change.

The parser reads these, but `apply_alter` rejects them. Accept them as no change: `SET (…)`, `REPLICA IDENTITY`, `OWNER TO`, `ENABLE`/`DISABLE TRIGGER`/`RULE`, `VALIDATE CONSTRAINT`.

A clause matching none of these still fails its table at `path:line`.

**Blocked by:** None

**Status:** resolved

- [x] Each form above leaves the table generating, with the expected column shape.
- [x] A statement mixing a recognised clause with an unknown one still fails the table at its line.

## Comments

Done on branch `rok-sqlc-0-2-1`. See CHANGELOG.md [0.2.1].
