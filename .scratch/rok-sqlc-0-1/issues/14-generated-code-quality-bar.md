# 14: Generated-code bar: documentation, lint gate, borrowed parameters, opt-in `serde`

**What to build:**
- **Documentation and lints:** every public generated item is documented. The fixture crates enforce `#![deny(missing_docs, warnings)]` plus clippy (`-D warnings` with a chosen set of pedantic lints) on generated code.
- **Borrowed parameters:** `Copy` keys stay by value. Lookups by text unique keys take `&str`, and list-shaped parameters take slices where the call doesn't need ownership. Inputs and patches stay owned.
- **`serde`:** `derive = ["serde"]` (config file, JSON payload, or a flag) adds `Serialize`/`Deserialize` to rows, typed ids, key structs, enums, inputs, and patches, and adds `serde` to the dependency hints.

Record the generated-code quality bar, with typed ids, as ADR 0013.

**Blocked by:** 13, 04

**Status:** ready-for-agent

- [ ] The fixture crate compiles under the lint gate, including the chosen pedantic lints, and the module fixture harness does too.
- [ ] With `serde` enabled, the fixture crate serializes a row to JSON and back (PostgreSQL seam or fixture test).
- [ ] Generated code without `serde` enabled doesn't mention `serde`.
- [ ] ADR 0013 is written.
