# 01: Follow `DROP CONSTRAINT` / `ADD CONSTRAINT … CHECK` in migrations

**What to build:** decision B3. Today `src/parser/postgres/recover.rs` drops `ALTER CONSTRAINT`-style clauses, and `DROP CONSTRAINT` isn't followed, so a CHECK that defines a value list can't change after its first migration.

- **`ALTER TABLE t DROP CONSTRAINT [IF EXISTS] name`** removes the named constraint from the model. That covers a CHECK, a unique key, a foreign key and a primary key, if the model tracks it by name. A name the model doesn't know is an error (or, with `IF EXISTS`, nothing). Implicitly named constraints follow PostgreSQL's default naming (`<table>_<column>_check`, `<table>_<cols>_key`, `<table>_pkey`, `<table>_<col>_fkey`); check what the model already records.
- **`ALTER TABLE t ADD CONSTRAINT name CHECK (…)`** adds it, so a value-list CHECK (`col IN (…)` / `col = ANY (ARRAY[…])`) regenerates the column's enum with the new values.
- The output changes only for schemas with such migrations. Add parser and generator tests: the `auth_tokens.purpose` widening scenario, a dropped unique key losing its lookup, a dropped FK losing its relation, and an unknown name erroring.
- Update `docs/guide/auth.md` (remove the limitation), the README's supported-SQL section, and the CHANGELOG (Unreleased).

**Blocked by:** None

**Status:** resolved

- [x] Tests above. Gates: fmt, clippy, `cargo test --locked`, `+1.94` against Debian, the `+1.88` build.

## Comments

**Supported.** `ALTER TABLE t DROP CONSTRAINT [IF EXISTS] name [CASCADE]` drops a primary key (columns stay `NOT NULL`), a unique constraint (its unique key and lookup go), a foreign key (single-column: its relationship goes; composite ones are kept by name only) or a check. Dropping a check recomputes its column's value list and ranges from the checks that remain, so drop + `ADD CONSTRAINT … CHECK (col IN (…))` replaces the enum's variants. An unknown name fails the table (`DROP CONSTRAINT: constraint `x` does not exist`), unless `IF EXISTS`. `RENAME CONSTRAINT` now renames any kind; `RENAME COLUMN` and `DROP COLUMN` carry through to checks. New model fields: `Table::named_constraints` (checks + composite FKs, with the check's SQL text), `ForeignKey::name`, `KeyConstraint::primary`; the constraint types moved to `src/schema/constraints.rs`.

**Implicit names** (verified against PostgreSQL 17): `<table>_pkey`, `<table>_<cols>_key`, `<table>_<cols>_fkey` (all FK columns joined by `_`), `<table>_<col>_check` when the check names exactly one column of the table, `<table>_check` otherwise; a number follows the label (`_check1`) while the name is taken by any constraint of the schema; parts are shortened to 63 bytes as PostgreSQL does. Checks are named before foreign keys, column checks before table checks.

**Not done:** `col = ANY (ARRAY[…])` is still not a value list (only `IN (…)`), to keep existing output unchanged; `DROP CONSTRAINT … CASCADE` doesn't drop other tables' FKs to a dropped key; constraints `CREATE TABLE` ignores (`EXCLUDE`) can't be dropped by name.

**For ticket 02** (`templates/*/migrations/0003_two_factor.sql`): the inline check in `0002_auth.sql` is `auth_tokens_purpose_check`, so write

```sql
ALTER TABLE auth_tokens DROP CONSTRAINT auth_tokens_purpose_check;
ALTER TABLE auth_tokens ADD CONSTRAINT auth_tokens_purpose_check CHECK (
    purpose IN ('email_verification', 'password_reset', 'email_change',
                'invitation', 'two_factor_pending')
);
```

`AuthTokenPurpose` gains `TwoFactorPending`; the generated code equals what declaring the wider list in `0002` would generate.
