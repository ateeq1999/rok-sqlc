# 03: Generate validation rules on inputs and patches

**What to build:** With `derive` containing `validate`, generate what ADR 0015's "Rules come from the schema" paragraph describes:
- `validator::Validate` joins the derive list of every create input and update patch, and only those.
- Each field whose column has rules (ticket 01) gets `#[validate(length(…))]` or `#[validate(range(…))]`, with an English message naming the column, e.g. `"name must be at most 80 characters"`, `"name must be between 1 and 80 characters"`, `"price must be at least 0"`. Messages count characters.
- Patch fields (`Option<T>` and `Option<Option<T>>`) are validated only when a value is present. validator 0.21 handles both.
- A typed id over text whose column has a length rule gets `impl validator::ValidateLength<u64> for XId`, delegating to the inner string, so the rule applies through the typed id. The same goes for a foreign key field using another table's typed id.
- The `db/mod.rs` dependency hints add `validator = { version = "0.21", features = ["derive"] }`.

The fixture app (`tests/fixture-app`) covers this: add a table with `VARCHAR(n)`, numeric and length `CHECK`s and a text key, generate with `validate`, and assert in a PostgreSQL-free test that `validate()` accepts and rejects the right values with the right messages and codes. Keep the fixture's lint gate green (ADR 0013: every public item documented).

**Blocked by:** 01, 02

**Status:** ready-for-agent

- [ ] Generator tests show the attributes, messages and typed-id impls, and show that output without `validate` is unchanged.
- [ ] The fixture app exercises `validate()` on an input and a patch, including `Some(None)` and absent fields.
