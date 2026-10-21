# 01: Follow `DROP CONSTRAINT` / `ADD CONSTRAINT … CHECK` in migrations

**What to build:** decision B3. Today `src/parser/postgres/recover.rs` drops `ALTER CONSTRAINT`-style clauses, and `DROP CONSTRAINT` isn't followed, so a CHECK that defines a value list can't change after its first migration.

- **`ALTER TABLE t DROP CONSTRAINT [IF EXISTS] name`** removes the named constraint from the model. That covers a CHECK, a unique key, a foreign key and a primary key, if the model tracks it by name. A name the model doesn't know is an error (or, with `IF EXISTS`, nothing). Implicitly named constraints follow PostgreSQL's default naming (`<table>_<column>_check`, `<table>_<cols>_key`, `<table>_pkey`, `<table>_<col>_fkey`); check what the model already records.
- **`ALTER TABLE t ADD CONSTRAINT name CHECK (…)`** adds it, so a value-list CHECK (`col IN (…)` / `col = ANY (ARRAY[…])`) regenerates the column's enum with the new values.
- The output changes only for schemas with such migrations. Add parser and generator tests: the `auth_tokens.purpose` widening scenario, a dropped unique key losing its lookup, a dropped FK losing its relation, and an unknown name erroring.
- Update `docs/guide/auth.md` (remove the limitation), the README's supported-SQL section, and the CHANGELOG (Unreleased).

**Blocked by:** None

**Status:** ready-for-agent

- [ ] Tests above. Gates: fmt, clippy, `cargo test --locked`, `+1.94` against Debian, the `+1.88` build.
