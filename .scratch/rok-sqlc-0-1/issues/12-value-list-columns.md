# 12: Value-list columns become enums

**What to build:** A text column constrained by `CHECK (col IN ('a', 'b', …))`, inline or as a table constraint, becomes a **value-list column**. It generates a Rust enum (in the shared `types` module, named after table and column) that encodes and decodes as text and keeps each exact value. It behaves like an enum column everywhere: rows, inputs, patches, filters, sort keys, and query inference. `CHECK`s of any other shape are ignored as before.

**Blocked by:** 11

**Status:** ready-for-agent

- [ ] A value-list column round-trips through create, get, and search against PostgreSQL, and an invalid value can't be constructed in Rust (PostgreSQL seam).
- [ ] A nullable value-list column becomes `Option<Enum>`.
- [ ] Two tables with the same column name and different value lists get distinct enum types.
- [ ] Corpus columns such as `business_type` generate enums, and the module fixture harness still compiles and runs.
