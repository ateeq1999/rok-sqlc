# Composite primary keys, array columns, and `TIME` are supported

Milestone 1 shipped models and reads for a scalar-only, single-column-key subset. Running the CLI over the 26-module ERP corpus showed that subset left 8 modules with nothing to generate and cost `core` three tables, so milestone 1.5 widens the supported surface: composite primary keys (declaration order preserved, every key column forced non-null), array columns of any supported scalar (`TEXT[]` → `Vec<String>`, `UUID[]` → `Vec<uuid::Uuid>`, `INTEGER[]` → `Vec<i32>`, nullability wrapping as `Option<Vec<T>>`), and `TIME` without time zone → `chrono::NaiveTime`. `get_by_id` now takes one parameter per primary key column, named after the column, and binds them in key order, so single-column and composite keys follow one rule rather than two.

Two things stay rejected on purpose. `NUMERIC`/`DECIMAL` waits for the rust_decimal milestone — the corpus uses it 111 times, and guessing a Rust type now would bake in a rounding story nobody has chosen. `TIME WITH TIME ZONE` stays unsupported: PostgreSQL itself discourages it and no fixture uses it.

*Superseded in part (0.2):* `TIME WITH TIME ZONE` now maps to SQLx's `PgTimeTz` (and `INTERVAL` to `PgInterval`, `BYTEA` to `Vec<u8>`), for legacy schemas that already use it. Neither SQLx type implements serde's traits, so with `derive = ["serde"]` such a column fails the run rather than generating code that doesn't compile.

## Considered Options

- **Materialize arrays into joined tables** — rejected: it invents structure the schema does not express.
- **Support `NUMERIC` as `f64` to unblock the corpus** — rejected: lossy for money, and the corpus is exactly where correctness matters.
- **Keep single-column keys and skip the tables that use composite keys** — rejected: `order_items`, `applied_migrations`, and `module_dependencies` are ordinary tables, not schema smells.
