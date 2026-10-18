# 09: `rok-sqlc check-arch`

**What to build:** A subcommand that checks a project against ADR 0016's architecture. It prints each violation as `path:line: rule: message` and exits 1 if any are found, for CI. It reads `rok-sqlc.toml` for `api.output` and `api.db_path`.

**Layout checks:**
- A generated feature directory holds only its eight files.
- No file may be named `utils.rs`, `helpers.rs` or `common.rs`.
- No unknown top-level directories under `src/`, other than `db`, `shared`, the features directory, and `bin` (configurable later).
- `--lenient` turns layout violations into warnings.

**Rules, by parsing Rust source:** use `syn` (it builds on Rust 1.88) to read `use` items and paths, not grep. Report only real code, not comments or strings.
1. `actix_web` is imported only in `routes.rs`, `handlers.rs`, `app.rs`, `main.rs`, `error.rs` and `shared/`.
2. `sqlx` is imported only in `repo.rs`, `model.rs`, `db/`, `error.rs`, `app.rs` and `shared/`.
4. A feature never refers to another feature's modules.
5. `shared/` never refers to the features module.
6. No `unwrap`, `expect`, `panic!`, `todo!`, `println!` or `dbg!` outside `#[cfg(test)]` and `tests/`.

Plus the cross-feature write rule: a feature may not call another table's `db::<table>::write` items, though reads are allowed.

**Blocked by:** 06

**Status:** ready-for-agent

- [ ] CLI tests on a scratch project, for each rule and layout check: positive, negative, and `path:line` output.
- [ ] The `tests/api-app` fixture passes `check-arch` cleanly, so generated code obeys the rules.
- [ ] README documents the subcommand (drift tests).
