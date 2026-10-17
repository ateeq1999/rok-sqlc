# 03: A managed column with no source of values fails the table

**What to build:** Managed columns are left out of inserts, so the database must supply their value. A managed column has no source of values when all of these hold:
- it is `NOT NULL`;
- it has no `DEFAULT`;
- it is not `GENERATED`, identity or serial;
- its table has no `BEFORE INSERT` (or `BEFORE INSERT OR UPDATE`) trigger in the migrations.

In that case the table fails with a `path:line` diagnostic. The diagnostic names the column and says how to fix it. Nullable managed columns are fine.

`CREATE TRIGGER` statements are currently ignored on purpose, so they need to be recorded now. Trigger detection works per table and does not inspect function bodies; document this in the ADR.

**Blocked by:** None

**Status:** resolved

- [x] At the CLI seam:
  - a NOT NULL managed column with no default and no trigger fails;
  - the same column with a default, an identity, or a BEFORE INSERT trigger generates.
- [x] Running the `rok` preset over the corpus changes `EXPECTED` only for tables that really have no source of values. The report lists any such tables.

## Comments

Resolved on integration branch `rok-sqlc-0-2`. Review fixes in 760d008..e2255b5, large-file refactor up to 6fa10e3, release prep in the 0.2.0 commit. See CHANGELOG.md [0.2.0] and the ADRs.
