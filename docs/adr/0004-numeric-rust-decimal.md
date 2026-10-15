# `NUMERIC`/`DECIMAL` map to `rust_decimal::Decimal`

`NUMERIC` and `DECIMAL` (with or without precision/scale) map to `rust_decimal::Decimal`, and the generated `db/mod.rs` dependency hints add the `rust_decimal` sqlx feature and the `rust_decimal = "1"` crate. This closes the last gap the ERP corpus exposed: after milestone 1.5 every remaining failure across the 26 modules was a `NUMERIC` column (113 of them), so the corpus is now fully generatable.

`rust_decimal` stores a 96-bit mantissa: 28 significant digits. A declaration whose precision exceeds that (`NUMERIC(50, 10)`) is rejected per ADR 0001 rather than silently generated into code that fails to decode at runtime. Bare `NUMERIC` (unbounded in PostgreSQL) is accepted on the assumption that real values fit; a value that overflows surfaces as a decode error, which is a data problem rather than a schema-shape problem.

## Considered Options

- **`f64`** — rejected: lossy for money, and the corpus is exactly where exactness matters.
- **`bigdecimal::BigDecimal`** — rejected for now: arbitrary precision, but heavier, and the corpus never exceeds 18 digits; revisit if a schema needs more than 28 significant digits.
- **Pass-through as `String`** — rejected: forces every consumer to parse before doing arithmetic.
