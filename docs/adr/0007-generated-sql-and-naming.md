# Generated SQL: schema-qualified, static where possible, `QueryBuilder` where not

Running the generated code against the migrated 26-module ERP corpus exposed a correctness bug: the parser dropped the schema qualifier, so `catalog.products` generated `FROM "products"`, which resolves through `search_path` and fails (or hits the wrong table) for every qualified table in the corpus. Tables now keep their schema (`Table::schema`) and every statement references `catalog.products`.

Generated statements follow three rules:

- **Static SQL whenever the shape is fixed.** `get_by_id`, `exists_by_id`, `list`, `list_page`, `count`, `delete`, and `create` for tables without defaulted columns are a single multi-line raw-string statement with an explicit column list and `.bind(...)` per placeholder.
- **`sqlx::QueryBuilder` when the shape depends on the input.** `update` (which columns are patched) and `create` with defaulted columns (which columns are omitted) push identifiers from the schema and values with `push_bind`, so SQLx allocates each placeholder as it binds the value. Placeholder numbers and bind order cannot drift apart, and no runtime value is ever formatted into SQL. These statements are not compile-time checked.
- **Identifiers are quoted only when PostgreSQL requires it**: anything that is not a plain lowercase identifier, plus the reserved, type/function-name, and column-name keyword categories from `pg_get_keywords()`. SQL stays readable (`price_cents`, not `"price_cents"`) and remains correct for `"select"`, `"Mixed Case"`, or `"1st"`.

The previous runtime placeholder arithmetic (`sets.len() + 1`) was in fact consistent, because it counted only the fields actually pushed, but it kept two independent sequences (SQL text and binds) in lockstep by convention. Delegating to `QueryBuilder` removes the convention. `tests/fixture-app/tests/postgres.rs` applies all 36 unchanged/set/set-NULL combinations of a four-column patch against PostgreSQL and checks every stored field.

The read side gains `exists_by_id`, `list_page(limit, offset)`, and `count`. `list` and `list_page` order by primary key, so pagination is stable; a table without a primary key gets an unordered `list` and `count` only. Row structs derive `PartialEq`, `UpdateX` derives `Default` and offers `is_empty()`, and `NewX` derives `Default` when every field is optional. Generated files are laid out to be rustfmt-stable, which the fixture test enforces with `rustfmt --check`.

## Struct names are singular

`products` now generates `Product`, `NewProduct`, and `UpdateProduct` (was `Products`, `NewProducts`, `UpdateProducts`). Only the head noun is singularized (`stock_levels` → `StockLevel`, `units_of_measure` → `UnitOfMeasure`) by a small rule set with irregular and uncountable lists; module names and SQL names are unchanged. This renames every generated type, a breaking change for consumers, accepted because the project is pre-1.0 and a row type named after one row reads correctly at every call site (`Option<Product>`, `Vec<Product>`).

## Identifiers in dynamic SQL

The relationship filters build `EXISTS` subqueries with `QueryBuilder`, and the shared `db::search::open_exists` and `correlate` push table names, aliases, and column names as SQL text (identifiers cannot be bound). They take only identifiers rok-sqlc generated and quoted (the `ident`/`table_ref` rules above, and the `_r<depth>` aliases), never caller input, and their doc comments say so. Every value is still bound.

## Considered Options

- **A generated `Patch<T>` enum instead of `Option<Option<T>>`** — rejected for now: ADR 0005's `Option` encoding is unchanged and documented on each nullable field; a `Patch` type would add a shared runtime file to every output directory and a second, larger API break.
- **Always quote identifiers** — rejected: correct but noisy, and the keyword categories make minimal quoting just as safe.
- **Build all statements with `QueryBuilder`** — rejected: static SQL is easier to review in generated diffs and is the shape `query_as!` would need if compile-time checking is added later.
- **Separate `*_in_transaction` functions** — rejected again (ADR 0006): the generic executor already accepts `&mut *tx`, and the PostgreSQL tests exercise rollback and commit through it.
- **Rename `delete` to `delete_by_id`** — rejected: not worth a second rename; `delete` already mirrors `create`/`update`.
