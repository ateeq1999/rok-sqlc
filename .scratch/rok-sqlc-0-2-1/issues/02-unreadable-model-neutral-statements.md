# 02: `BEGIN ATOMIC` bodies and other statements that can't change the model

**What to build:** When a migration file doesn't parse as a whole, it is split at semicolons. Make that split treat a `BEGIN ATOMIC … END` body (with `CASE … END` inside it) as part of its statement.

A statement the parser can't read stops the run today. Skip it instead when it can't change the model: `CREATE [OR REPLACE] FUNCTION`, `CREATE [OR REPLACE] PROCEDURE`, `DO`, and any form of `ALTER SEQUENCE`, `ALTER VIEW`, `ALTER MATERIALIZED VIEW` or `ALTER INDEX` that isn't a rename (renames are ticket 03). Any other unreadable statement still stops the run at its `path:line`.

**Blocked by:** None

**Status:** resolved

- [x] A migration with a `BEGIN ATOMIC` function and a table after it generates the table.
- [x] An unreadable `CREATE TABLE` still stops the run at its line.

## Comments

Done on branch `rok-sqlc-0-2-1`. See CHANGELOG.md [0.2.1].
