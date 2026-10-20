# 02: `tests/api-app-axum` wire parity, and `check-arch` per framework

**What to build:** decision A7.

- **`tests/api-app-axum`:**
  - Generated with `framework = "axum"` from the same migrations and config as `tests/api-app` (share them rather than copy where the harness allows).
  - Port `tests/api-app`'s PostgreSQL suites (shapes, references, unique, rules, relations, owner/tenant scoping, versioning/ETag, pagination, errors) so they assert the same statuses and bodies.
  - Factor shared assertions to keep the two suites from drifting, for example a shared test module driven over an HTTP client against either app, if that's practical. Otherwise mirror file by file and say so.
  - Wire it into `tests/fixture_compile` like `api_app.rs`.
- **`check-arch`:** detect the project's framework from `rok-sqlc.toml`. Under axum, the handler and route rules accept `axum::…` and forbid `actix_web::…`; under actix, the reverse. Every other rule stays the same. Add rule tests for both.

**Blocked by:** 01

**Status:** resolved

- [x] The axum fixture suite passes against PostgreSQL with the same answers as actix.
- [x] `check-arch` rule tests for both frameworks.
- [x] Gates pass.

## Comments

Done on `rok-sqlc-0-7-02`.

**Shared suites, not mirrored.** The suites moved from `tests/api-app/tests/` to `tests/api-suite/` (access, docs, documents, health, lookups, notes, openapi, references, relationships, rules, schemas, shapes, unique) and are framework-free. `tests/api-suite/client.rs` holds the client: `Database`, `Answer`, `With`, `call*`, `create`, `assert_error`, `token`, `fetch`, `document`, and the `Request`/`Response` types. Each fixture has a small `tests/common/mod.rs` adapter: the framework's `Method`, `StatusCode`, `api_test` (`actix_web::test` or `tokio::test`), `send(pool, secret, Request) -> Response` (actix `init_service`, or axum `router(AppState)` + `tower::ServiceExt::oneshot`; a tenant goes in the request extensions), and `status_of(&AppError)`. Each fixture's `tests/<suite>.rs` is three lines: `mod common;` and `#[path = "../../api-suite/<suite>.rs"] mod <suite>;`. The axum ones add `extern crate rok_sqlc_api_app_axum as rok_sqlc_api_app;` so the suites name one crate. `shared.rs` (which tests extractors directly) stays per framework, and axum's adds `Path` message tests for one value, a tuple and named fields.

**One OpenAPI document.** `openapi.rs` compares both apps against `tests/api-app/tests/expected/openapi.json`. `info.title` is the package name, so the test asserts it equals `CARGO_PKG_NAME` and then sets it to `rok-sqlc-api-app` before comparing. Apart from the title, the two documents are byte-identical.

**The fixture.** `tests/fixture_compile/api_app.rs` now generates either fixture: `generate_api_app(framework)`, `fixture_dir`, `lock`, plus `passes_the_lint_gate` / `passes_its_tests` / `passes_check_arch` / `is_rustfmt_clean`. `api_app_axum.rs` runs those for axum. The axum fixture reads `tests/api-app/migrations/` directly (it isn't copied), uses the same settings with `framework = Axum` and `extractors = [axum]`, and is scaffolded with `init --framework axum`. `features/drafts/service.rs` is copied from `tests/api-app` before generating, since a service is the same for both frameworks. The generated files are git-ignored. `tests/api-app-axum` gained `rok-sqlc.toml`, `src/lib.rs`, and the dev-dependency `tower` (`util`). That was a one-line `Cargo.lock` change, as `tower` was already in the lock.

**Wire parity on `400`s.** The generated axum `shared/validation.rs` now words path and query failures the way actix does. For `Path`: `Path deserialize error: ` followed by the value's failure, where axum's `ParseError*` kinds become `can not parse "abc" to a i64` (actix-router's wording) and `Message`/`DeserializeError` pass through. For `Query`: `Query deserialize error: ` followed by the `serde_urlencoded` error, taken from the rejection's source chain (`serde_path_to_error::Error<serde::de::value::Error>`), without the field path axum adds. Any other rejection kind falls back to `body_text()`. Only wiring bugs reach that fallback (wrong number of params, unsupported types). `notes.rs`'s `a_malformed_path_or_query_says_why` asserts the exact messages for a UUID key, a bigint key, a composite key's second segment, `?page=abc` and `?order=up` against both apps. No case needed the status-and-code-only fallback.

**Generator fixes the lint gate surfaced (axum only).** `register_services` is `#[must_use]`, and `readiness` no longer trips `single_match_else`. Expected files in `tests/api_axum/` were regenerated: `generated.rs`, `health.rs` and `validation.rs` changed. Actix output is unchanged.

**check-arch.** `Project::framework` comes from `[api] framework`. Rule 1 names that framework's crate (`rules(framework)`, `rules_markdown(framework)`, `crate_name`), so `init --framework axum`'s `AGENTS.md` now says `axum` and the init CLI test no longer exempts it. The other framework's crate is a `rule-1` violation everywhere except the data layer, whose `extractors` may serve both: "`actix_web::…`: the project uses axum (`[api] framework`), so nothing but db/ may use actix_web". Rule texts other than rule 1's crate name are unchanged. `docs/guide/architecture.md` still states the actix rule, and its test checks actix only; the docs are ticket 05's. Tests: `tests/cli/check_arch_axum.rs` (axum passes; axum outside routes/handlers; actix_web in an axum project; axum in an actix project; `init --framework axum` states rule 1 for axum and passes check-arch) and `rules.rs` unit tests.

**Merged `rok-sqlc-0-7` (ticket 03).** `templates/axum-api/AGENTS.md`'s rule 1 already matched `rules(ApiFramework::Axum)` word for word. A new unit test, `each_templates_agents_md_states_its_frameworks_rules`, now pins both templates' `AGENTS.md` to their framework's rules. The template's generated `features/generated.rs`, `shared/health.rs` and `shared/validation.rs` were regenerated (`ROK_SQLC_UPDATE_EXPECTED=1`) for this ticket's generator changes.
