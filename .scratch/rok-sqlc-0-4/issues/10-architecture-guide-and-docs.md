# 10: The architecture guide and milestone docs

**What to build:**

- **`docs/guide/architecture.md`:** the maintainer's guidelines (`notes/architecture-guidelines-original.md`), with ADR 0016's amendments applied throughout, and every rule matching what `check-arch` enforces. Code samples must come from compiled sources. Use the `tests/api-app` fixture or a small `docs/examples/api` built with `init` plus generation, and add it to the README drift tests (`tests/readme.rs`).
- **`docs/guide/layout.md`:** which files are generated, scaffolded or yours, and what regeneration does to each.
- **`docs/guide/index.md`:** lists the pages planned for milestone 3 as "coming".
- **README:** a new "Generated APIs" section linking the guide, documenting `[api]`, `init` and `check-arch`, with samples from compiled sources.
- **CHANGELOG `[Unreleased]`:** every user-visible change from tickets 01 to 09, with ticket 01 under breaking changes.
- **Other docs:** ADR 0012 lists `ApiConfig` and its enums as stable API.

**Blocked by:** 08, 09

**Status:** resolved

- [x] README drift tests pass, including the new guide samples.
- [x] The full suite passes, including PostgreSQL, the lint gates, and `check-arch` on the fixture.

## Comments

Resolved on branch `rok-sqlc-0-4-10`.

- **Example:** `docs/examples/api`, made with `rok-sqlc init` plus a migration (`projects`, `tasks`, both owned), a query file (`OpenTask`) and `rok-sqlc g`, all checked in. It has a business-rule override (`tasks` `create_in`) and a port (`ProjectTasks`, declared in `tasks/service.rs`, implemented in `app.rs` over `ProjectsService::open_task_in`, built in `TasksService::new`). `tests/readme.rs` adds it to the example crates (drift check, clippy) and runs `check-arch` on it.
- **Drift tests:** `tests/readme.rs` now reads the README and every `docs/guide/*.md` page (source, tree and config-sample checks), requires `index.md`, `architecture.md` and `layout.md`, and requires the architecture page to have Rust samples from `docs/examples/api`.
- **Docs:** `docs/guide/{index,architecture,layout}.md`; README "Generated APIs" section; CHANGELOG `[Unreleased]` gains the ticket 07 entry (missing before), fixes 03's "take effect later", and a docs entry; ADR 0012 lists `ApiConfig`, `ApiAccess`, `ApiEndpoint`; ADR 0016 matches what was built (check-arch allowances, `XService::new(pool)` and ports, owned writes lock then update, `search_endpoint` and the `/search` body, sync from one side and not to owned tables, hidden filters rejected, `init` without templates).
- **Deviation:** a binary crate using the generated code gets `dead_code`/`unused_imports` warnings; the example allows both in `Cargo.toml` and the layout page says so. The scaffolded `main.rs` could allow them itself (follow-up).
