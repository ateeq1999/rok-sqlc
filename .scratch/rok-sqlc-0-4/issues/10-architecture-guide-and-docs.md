# 10: The architecture guide and milestone docs

**What to build:**

- **`docs/guide/architecture.md`:** the maintainer's guidelines (`notes/architecture-guidelines-original.md`), with ADR 0016's amendments applied throughout, and every rule matching what `check-arch` enforces. Code samples must come from compiled sources. Use the `tests/api-app` fixture or a small `docs/examples/api` built with `init` plus generation, and add it to the README drift tests (`tests/readme.rs`).
- **`docs/guide/layout.md`:** which files are generated, scaffolded or yours, and what regeneration does to each.
- **`docs/guide/index.md`:** lists the pages planned for milestone 3 as "coming".
- **README:** a new "Generated APIs" section linking the guide, documenting `[api]`, `init` and `check-arch`, with samples from compiled sources.
- **CHANGELOG `[Unreleased]`:** every user-visible change from tickets 01 to 09, with ticket 01 under breaking changes.
- **Other docs:** ADR 0012 lists `ApiConfig` and its enums as stable API.

**Blocked by:** 08, 09

**Status:** ready-for-agent

- [ ] README drift tests pass, including the new guide samples.
- [ ] The full suite passes, including PostgreSQL, the lint gates, and `check-arch` on the fixture.
