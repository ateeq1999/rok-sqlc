# 05: Docs, review fixes, and 0.5.0

**What to build:**
- The guide: `configuring.md` gets a validation section covering automatic `unique`/`exists`, `[api.rules]` with every rule and transform, and examples. `wire-standard.md` lists every detail code (`unique`, `exists`, each rule's code, `immutable`, the schema-derived `length`/`range`). `troubleshooting.md` covers the new config errors, with the binary's real output.
- ADR 0016: a short section on validation rules, or a new ADR 0017 if the decisions warrant one (they changed `409` to `422`, which is a real trade-off).
- 0.5.0: version bump everywhere (precedent: commit a191d97), CHANGELOG `[0.5.0]` (breaking changes first), the README roadmap (validation moves from Planned to done), and `cargo publish --dry-run --locked`.

Do NOT tag or publish.

**Blocked by:** 01, 02, 03, 04

**Status:** ready-for-agent

- [ ] The drift tests pass, and the full suite passes on Rust 1.94 against Debian PostgreSQL.
- [ ] The 1.88 build and the publish dry-run pass.
