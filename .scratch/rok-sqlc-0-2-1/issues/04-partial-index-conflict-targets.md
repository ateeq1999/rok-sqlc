# 04: Partial-index `ON CONFLICT` targets in query files

**What to build:** Accept `ON CONFLICT (cols) WHERE <predicate> DO …` in query files. The parser can't read the `WHERE` there, so remove it from the tokens before analysis and keep the SQL as written for the generated function.

A target with a predicate matches:
- a full unique key over those columns;
- a partial unique index over those columns whose condition is the same predicate.

PostgreSQL infers any index the predicate implies. rok-sqlc checks only an exact match.

A placeholder in the predicate is an error. Without a predicate, a target that matches only a partial index fails with a hint to add `WHERE <condition>`.

The soft-delete convention's `WHERE deleted_at IS NULL` unique indexes can then be upsert targets.

**Blocked by:** None

**Status:** resolved

- [x] An upsert on a soft-delete table's partial unique index generates and runs against PostgreSQL.
- [x] The no-predicate error names the partial index's condition.

## Comments

Done on branch `rok-sqlc-0-2-1`. See CHANGELOG.md [0.2.1].
