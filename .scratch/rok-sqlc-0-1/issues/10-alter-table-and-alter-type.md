# 10: `ALTER TABLE` / `ALTER TYPE`: apply, ignore on purpose, or fail

**What to build:** Migrations that change tables after creating them produce models matching the final shape.
- **Applied:** `ADD COLUMN`, `DROP COLUMN`, `RENAME COLUMN`, and `ADD CONSTRAINT` (primary key, unique, foreign key).
- **Ignored on purpose:** row-level security (`ENABLE ROW LEVEL SECURITY`), `CREATE POLICY`, non-unique indexes, grants, triggers, comments, function definitions, and `SELECT fn(…)` calls.
- **Failing:** any other `ALTER TABLE` that targets a known table, with a `path:line` diagnostic naming the unsupported clause.
- **Recorded:** unique indexes on plain columns, as input to unique keys (ticket 15).

**Blocked by:** 03

**Status:** ready-for-agent

- [ ] A migration that adds a column, renames another, and drops a third produces a model the fixture crate can create and read back against PostgreSQL with the final columns (PostgreSQL seam).
- [ ] A foreign key added by `ALTER TABLE … ADD CONSTRAINT` is recorded like an inline one.
- [ ] RLS, policy, index, grant, trigger, comment, and function statements generate without warnings.
- [ ] An unsupported `ALTER TABLE` (for example `ALTER COLUMN … TYPE`) on a known table fails with a `path:line` diagnostic (parser diagnostics seam); on an unknown table it is ignored.
- [ ] The module fixture harness still passes; the corpus's `ALTER TABLE … ADD CONSTRAINT` foreign key is now recorded.
