# 01: Keep column lengths and simple `CHECK` rules on the model

**What to build:** The parser drops what ADR 0015 needs. Keep it on `Column` (`src/schema/types.rs`):
- the declared length of `VARCHAR(n)`, `CHARACTER VARYING(n)`, `CHAR(n)`, `CHARACTER(n)`, through `CREATE TABLE`, `ADD COLUMN` and `ALTER COLUMN … TYPE`. A type change clears it or replaces it.
- single-column `CHECK`s against literals, from `CREATE TABLE` (inline and table-level) and `ALTER TABLE … ADD CONSTRAINT`:
  - `col >= n`, `col > n`, `col <= n`, `col < n`, and the same with the literal on the left;
  - `col BETWEEN a AND b`;
  - an `AND` of these on one column;
  - `char_length(col)`, `character_length(col)` or `length(col)` compared with an integer literal, which gives a length bound.

  Strict bounds on integers become inclusive (`> 0` becomes min 1). On other numbers, a strict bound is kept only if it can be represented exactly; otherwise it is dropped. Record the decision in a comment.

Every other `CHECK` is ignored, as today. Value-list `CHECK`s keep becoming enums. Renaming a column keeps its rules, and dropping a column drops them.

This adds no generated output on its own: output without the 0.3 keys must be unchanged.

**Blocked by:** None

**Status:** ready-for-agent

- [ ] Parser unit tests cover each shape, including ALTER paths, plus shapes that must stay ignored (two columns, a function other than the length ones, `OR`).
- [ ] The whole existing suite passes unchanged.
