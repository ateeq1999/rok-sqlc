# 06: Release 0.3.0

**What to build:** Prepare 0.3.0:
- bump the version to 0.3.0 (Cargo.toml and Cargo.lock, the README status line, install script examples, the release workflow's example tag);
- add a CHANGELOG `[0.3.0]` section with every user-visible change from tickets 01 to 05, and update the compare links;
- add a README section "Validated bodies and HTTP extractors" with a short axum and actix-web handler example, the error envelope, and the rules table. Code samples must come from a `docs/examples/` directory that is generated and compiled, per the README drift tests. Add a new example if needed.
- add `Extractor` to the stable API list in the README and ADR 0012;
- run `cargo publish --dry-run`.

Do NOT publish or tag. The maintainer does that.

**Blocked by:** 01, 02, 03, 04, 05

**Status:** ready-for-agent

- [ ] The full suite, including PostgreSQL and the README drift tests, passes on the integration branch.
- [ ] `cargo publish --dry-run` passes.
