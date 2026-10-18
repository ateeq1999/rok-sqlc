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

**Status:** resolved

- [x] CLI tests on a scratch project, for each rule and layout check: positive, negative, and `path:line` output.
- [x] The `tests/api-app` fixture passes `check-arch` cleanly, so generated code obeys the rules.
- [x] README documents the subcommand (drift tests).

## Comments

Resolved on branch `rok-sqlc-0-4-09`.

**Command.** `rok-sqlc check-arch` (alias `a`), flags `-c/--config FILE` (else discovers `rok-sqlc.toml`) and `-l/--lenient`. The project directory is the config file's; it checks `<project>/src/`. Needs `api.output` (error otherwise); `api.db_path` must start with `crate::`. Violations go to stdout as `path:line: rule: message` (path relative to the project, `/`-separated; layout violations use line 1), sorted by path and line; any exits 1. Under `--lenient`, layout violations go to stderr as `warning: …` and don't fail. Code: `src/check_arch/` (`mod.rs` the layout and the rules, `source.rs` the `syn` visitor). Deps: `syn` 2 (`full`, `visit`) and `proc-macro2` (`span-locations`, for lines); builds on 1.88.

**Final rule set** (feature = a directory directly under `api.output`; db = the `api.db_path` directory; test code — `#![cfg(test)]` files, `#[cfg(test)]` items, `#[test]`/`#[*::test]` items — is exempt from every source rule; `tests/` is never read):
- `layout`: a feature directory holds only `mod.rs`, `routes.rs`, `handlers.rs`, `dto.rs`, `model.rs`, `repo.rs`, `service_api.rs`, `service.rs` (no other file, no subdirectory); no file or directory named `utils`, `helpers`, `common` (`.rs` or not) anywhere under `src/`; top-level directories under `src/` only `bin`, `shared`, the db path's first component, the features directory's first component.
- `rule-1` (`actix_web`): a feature's `routes.rs`, `handlers.rs`; `src/app.rs`, `src/main.rs`, `src/error.rs`; `<api.output>/generated.rs`; `src/shared/**`; `src/db/**`.
- `rule-2` (`sqlx`): a feature's `repo.rs`, `model.rs`; a feature's `service.rs` and `service_api.rs` only for exactly `sqlx::PgPool` and `sqlx::PgConnection`; `src/app.rs`, `src/error.rs`; `<api.output>/generated.rs`; `src/shared/**`; `src/db/**`. (Not `main.rs`.)
- `rule-4`: a file of feature `a` names `crate::<features>::b…` (also via `super`/`self`/a `use` binding; `generated` in the module path is skipped) where `b` is another feature directory. Root items (`crate::features::configure_all`) are fine.
- `rule-5`: a file under `src/shared/` names anything under `crate::<features>`.
- `rule-6`: method calls `unwrap()`/`expect()`, macros `panic!`, `todo!`, `println!`, `dbg!` (also inside macro arguments that parse as expressions).
- `cross-feature-write`: a file of feature `a` names `<db_path>::<t>::write…` with `t != a` (reads and `<db_path>::<a>::write` are fine).

Paths come from `use` trees, every `syn::Path` (expressions, types, patterns, multi-segment attributes like `#[actix_web::main]`), `#[derive(...)]` lists and `extern crate`; one-segment attributes (`#[sqlx(...)]`, `#[serde(...)]`) are helpers, not crate references. A path through a `use` binding that already breaks the rule is reported once, at the `use`.

**Deviations from the ticket text.** Rule 1 also allows `features/generated.rs` and `db/` (the generated wiring and `db/http/actix_web.rs`); rule 2 also allows `features/generated.rs` and the `PgPool`/`PgConnection` exception for `service.rs`/`service_api.rs`, which the generated trait and the scaffolded service need (ticket 04's open question, settled here). Test code is exempt from all source rules, not only rule 6. Macro bodies that don't parse as expressions are not read.

**AGENTS.md** (`src/init/templates/AGENTS.md`) now states exactly these allowed files per rule, and `src/init/tests.rs::agents_md_states_the_rules_check_arch_enforces` pins the text (and checks the layout tree lists every `FEATURE_FILES` entry).

**Tests.** `tests/cli/check_arch.rs`: a clean scratch project, each rule positive and negative with exact `path:line` output (comments/strings/tests ignored, binding dedup), layout checks, `--lenient`/`-l`, a non-default `api.output`/`db_path`, a missing `api.output`. `tests/fixture_compile.rs::api_app_passes_check_arch` runs it on the generated `tests/api-app` (new checked-in `tests/api-app/rok-sqlc.toml`, `[api] output = "src/features"`, read only by check-arch); `init_then_generate_builds_end_to_end` also runs `check-arch` after `init` + `g`. README documents the subcommand, alias and rules; CHANGELOG `[Unreleased]` entry.
