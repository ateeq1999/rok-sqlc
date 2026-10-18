# The maintainer's architecture guidelines, as given (2026-10-18)

This is the source for `docs/guide/architecture.md`. ADR 0016 records the amendments agreed in the design session. Apply them when writing the guide:

- **The data layer:** `src/db/` is the generated data layer for the whole schema. It replaces `db.rs` and is exempt from rules 2 and 4, and `db/pool.rs` holds pool creation and health.
- **Feature files:** a generated feature has eight files (`mod`, `routes`, `handlers`, `dto`, `model`, `repo`, `service_api`, `service`). `model.rs` and `repo.rs` re-export from `db`.
- **Cross-feature access:** a feature may read another table through `crate::db`. It writes another feature's data only through a port to that feature's service `_in` method, inside the caller's transaction.
- **Wire standard:** the axum starter's OpenAPI document, so no `{success, data, meta}` envelope. `shared/response.rs` holds the error body and paging types.
- **Pagination:** offset paging with a total (`page`/`per_page`), as the standard requires. Keyset stays available in `db`.
- **Health:** `/health/live` and `/health/ready`.
- **Rule checks:** `rok-sqlc check-arch` replaces `scripts/check-arch.sh`, which has a subshell bug: violations found inside `while` loops never fail the script.
- **Tooling:** `cargo xtask new-feature` becomes `rok-sqlc` generation plus `init`.

---

The original text, verbatim:

# Architecture Guidelines

A simple, restricted architecture for Rust (Actix Web + sqlx + Postgres) services.
**One crate, folders by feature, a fixed set of files per feature, six rules.**
Humans and code generators must follow this document exactly. If a rule gets in your way, change the rule here first, then change the code.

## 1. Principles

1. **Boring and predictable.** Every feature looks the same. If you know one feature, you know all of them.
2. **Modular monolith.** One deployable binary, strict feature boundaries. A feature can be moved into its own crate or service later without rewriting it.
3. **Stateless runtime.** The API can run as many identical copies as needed.
4. **Slow work goes to a queue.** Requests stay fast; failures in side effects never fail a request.
5. **Rules are checked by tooling, not by memory.** CI rejects violations.

## 2. Layout

```
src/
├─ main.rs              # starts the server. Nothing else.
├─ app.rs               # builds state, registers services, mounts every feature (the ONLY wiring file)
├─ config.rs            # reads env vars into one Config struct
├─ error.rs             # AppError enum + HTTP mapping (shared by all features)
├─ db.rs                # pool creation, migration runner
├─ shared/              # tools with no business meaning
│  ├─ pagination.rs  response.rs  validation.rs  auth.rs  jwt.rs  password.rs  mail.rs  storage.rs  queue.rs
└─ features/
   ├─ mod.rs            # one `pub mod <name>;` line per feature
   └─ <feature>/
      ├─ mod.rs         # re-exports: `routes`, the service, any ports. Nothing else is public
      ├─ routes.rs      # URL -> handler, scope-level middleware
      ├─ handlers.rs    # HTTP in/out only
      ├─ dto.rs         # request/response structs + validation attributes
      ├─ service.rs     # business rules
      ├─ repo.rs        # SQL functions
      └─ model.rs       # structs mirroring DB rows
migrations/  tests/  worker/  AGENTS.md
```

Not allowed: new top-level folders, new files inside a feature, `utils.rs`, `helpers.rs`, `common.rs`. If something does not fit, it is either a `shared/` tool (no business meaning) or it belongs in a feature. Decide which.

## 3. Where does each piece of code go?

| I am writing... | It goes in | Never in |
|---|---|---|
| New URL | `routes.rs` | handlers |
| Reading the request, calling the service, returning JSON | `handlers.rs` | service |
| Request/response shape, `#[validate(...)]` | `dto.rs` | model |
| A business rule | `service.rs` | handler, repo |
| SQL query | `repo.rs` | service, handler |
| Struct matching a DB table | `model.rs` | dto |
| New DB table or column | `migrations/000N_name.sql` + `model.rs` + `repo.rs` | anywhere else |
| Error case | variant in `error.rs` | feature folder |
| Code two features need | `shared/` | one feature |
| Call to an outside system | `shared/<name>.rs` behind a trait | service directly |
| Env variable | `config.rs` | `std::env` anywhere else |
| Background/slow work | outbox row from `service.rs`, handled by `worker/` | request path |
| Register a feature | `features/mod.rs` + `app.rs` | — |

## 4. The six rules

1. Only `routes.rs` and `handlers.rs` import `actix_web`.
2. Only `repo.rs` and `db.rs` import `sqlx`.
3. `service.rs` never writes SQL. It calls `repo.rs` functions and traits from `shared/`.
4. A feature never imports another feature's files. It may call another feature only through a trait (a *port*) that it declares itself.
5. `shared/` never imports `features/`.
6. No `unwrap`, `expect`, `panic!`, `todo!`, `println!`, `dbg!` in non-test code. Every fallible function returns `Result<_, AppError>`.

Conventions: handlers are thin (parse, call one service function, wrap the result; no `if` business logic). One service function is one use case, named as a verb. Repo function names say what they do. DTOs are separate from models; never return a model struct that contains secrets. Money is integer minor units, time is UTC, and IDs are UUIDs.

## 5. Templates

Section 5 of the original has templates for `model.rs`, `dto.rs`, `repo.rs` (taking `impl PgExecutor<'_>`), `service.rs` (a struct with a pool, a transaction, and an outbox enqueue in the same transaction), `handlers.rs`, `routes.rs` (one `web::scope` per prefix), `mod.rs`, and `error.rs` (one `AppError` with `NotFound`, `Conflict`, `Unauthorized`, `Forbidden`, `Validation`, `Internal(anyhow)`, implementing `ResponseError`; internal errors are logged and generic).

## 6. Ports

Never `use crate::features::users::...` from `projects`. Declare a `#[async_trait] pub trait OwnerLookup: Send + Sync` in your own service file. `app.rs` provides the implementation. Use traits only for outside systems and cross-feature ports, and never around repo functions.

## 7. Testing

- **Service rules:** `#[sqlx::test]` against a real Postgres, with fakes for outside traits.
- **Repo:** `#[sqlx::test]`.
- **API:** `tests/<feature>.rs` with `actix_web::test`.
- **Shared:** unit tests.

A feature is done with one service test per business rule and one API test per route (the happy path plus one failure).

## 8. Runtime checklist

- Stateless.
- Config from env only.
- Explicit pool size and timeouts.
- `/health` and `/ready`.
- Cursor/keyset pagination by default (amended: offset per the standard).
- `Idempotency-Key` on retried mutations.
- Migrations as a separate step, kept backward compatible.
- Structured logs with a request id.
- An outbox table written in the same transaction, processed by `worker/` with `FOR UPDATE SKIP LOCKED`.

## 7a. Adding a feature

Copy the golden feature, add a migration, register it in `features/mod.rs` and `app.rs`, add error variants, write tests, and run the checks.

## 9. Automated checks

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and `./scripts/check-arch.sh`, which checks rules 1, 2, 4, 5 and 6 by grep; this is replaced by `rok-sqlc check-arch`.

## 10. When to split

- More than about 5 developers, or slow builds: a `core` crate plus one crate per feature.
- Background work: add a `worker/` binary.
- Different scaling needs: extract the feature behind its port.

Otherwise, stay as you are.

## 11. Do and don't

**Do:** copy the golden feature, keep handlers short, write a new migration for every schema change, and return `Result` everywhere.

**Don't:** create `utils`/`helpers`/`common`, add a speculative trait, read env outside `config.rs`, call another feature's repo, do slow work in a request, or edit an applied migration.
