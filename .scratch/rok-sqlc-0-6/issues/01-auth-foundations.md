# 01: Auth foundations: schema, security tools, config, `AuthUser`, `check-arch`

**What to build:** what every later flow needs (decisions Q3, Q4, Q8, Q10, Q19, Q21, Q22, Q23).

**Schema (Q10):**
- Template migration `0002_auth.sql`:
  - `users.token_version INTEGER NOT NULL DEFAULT 1`;
  - `auth_tokens`: id, user_id → users ON DELETE CASCADE, `purpose` with a CHECK in (`email_verification`, `password_reset`, `email_change`, `invitation`), `token_hash` UNIQUE, `new_email`, `expires_at`, `used_at`, `created_at`, and an index on (user_id, purpose);
  - `outbound_mail` (id, payload, status `pending`|`dead`, attempts, `next_attempt_at`, last_error, timestamps).
- Template `rok-sqlc.toml`: add `token_version` to `managed_columns`, `auth_tokens` and `outbound_mail` to `[api] exclude`, and `queries = ["queries/"]`.

**Query files (Q8):** a `queries/` directory in the template, starting with `queries/auth.sql`, holding the queries this ticket needs (load the caller's `token_version`/`is_active`/`role`, find by email for login). Later tickets add theirs. Regenerate the template's `src/db`.

**`check-arch` (Q22):** a feature may call `db::queries::<its own feature name>` (and nested paths), and calling another feature's query module is reported. Add rule tests, and update `docs/guide/architecture*.md`.

**Security tools (Q4, Q3), in `shared/security/`:**
- `password`: argon2id hash and verify inside `spawn_blocking`, the 8–128 policy, and the dummy hash for unknown emails.
- `token`: 32 random bytes, base64url, stored as SHA-256 hex.
- `jwt`: issue and decode, with claims `sub`, `role`, `tv`, `exp`.
- Move today's `shared/auth.rs` there, or keep it, whichever reads better.

**`AuthUser` and `AdminUser` (Q21):** decode the token, then make one query loading `token_version`, `is_active` and `role` for a live user. A mismatched `tv`, an inactive account or a soft-deleted account gets `401`. `AdminUser` takes the role from the database. This now needs the pool from app data, so update `app.rs` wiring and any tests that build tokens.

**Config (Q23):** add `JWT_TTL`, `REQUIRE_VERIFIED_EMAIL`, `APP_URL`, `MAIL_TRANSPORT`, `SMTP_URL`, `MAIL_FROM`, `BREACH_CHECK`, `TRUST_PROXY` and `AUTH_RATE_LIMIT`, with development defaults, all documented in the template's `.env.example` (create it if it's missing). Read them only in `config.rs`.

**Dependencies:** add `argon2`, `rand`, `sha2` and `base64` to `Cargo.toml.template` (it may already have a JWT crate). Pin versions that compile on Rust 1.94.

**Blocked by:** None

**Status:** resolved

- [x] Unit tests: hashing and verify, the policy, token hashing, JWT round trip, and a stale `tv` rejected.
- [x] Integration (PostgreSQL): an inactive user's valid token → 401; a role change applies without re-login.
- [x] `check-arch` rule tests. The template stays `check-arch` clean, clippy-clean on 1.94, and its contract test passes.

## Comments

**Implemented on `rok-sqlc-0-6-01`.** What tickets 02–05 build on:

- **Query files.** `rok-sqlc.toml` has `queries = ["queries/"]`, so every `templates/actix-api/queries/<stem>.sql` becomes `src/db/queries/<stem>.rs` (module `crate::db::queries::<stem>`), declared by a generated `src/db/queries/mod.rs`. Add a file per feature (`queries/mail.sql`, `queries/account.sql`, more queries in `queries/auth.sql`), then regenerate the checked-in output with `ROK_SQLC_UPDATE_EXPECTED=1 cargo test --test fixture_compile actix_template_checks_in` (it runs `init --template actix` + `g` and copies every `@generated` file back). Annotations per ADR 0008: `-- name: Foo :one|:maybe_one|:many|:exec|:execrows`; `lower($1)` needs `-- param: $1 email String`; a result that isn't a whole table gets a `FooRow` struct; a `users.id` parameter is typed `UserId` (`uuid.into()`).
- **`check-arch` (Q22):** new rule `cross-feature-query`. A feature may call `db::queries::<its own name>` (nested paths too); calling `db::queries::<another existing feature>` is reported. A module named after no feature directory, and `shared/`, are unrestricted. So `queries/auth.sql` is the `auth` feature's, and `shared/auth.rs` uses it; the `account` feature needs its own `queries/account.sql`. Rule 4's text (AGENTS.md, architecture guide) says so.
- **`queries/auth.sql` today:** `session_state(executor, UserId) -> Option<SessionStateRow { token_version: i32, is_active: bool, role: Role }>` (live accounts only) and `find_login(executor, &str) -> Option<FindLoginRow { id, password_hash: Option<String>, role, is_active, email_verified, token_version, failed_logins, locked_until }>` (case-insensitive, live accounts).
- **Schema (`migrations/0002_auth.sql`):** `users.token_version INTEGER NOT NULL DEFAULT 1` (managed and hidden); `auth_tokens(id, user_id ON DELETE CASCADE, purpose TEXT CHECK in 4 values, token_hash UNIQUE, new_email, expires_at, used_at, created_at DEFAULT now())` + index `(user_id, purpose)`; `outbound_mail(id, payload JSONB, status 'pending'|'dead' DEFAULT 'pending', attempts INT DEFAULT 0, next_attempt_at DEFAULT now(), last_error, created_at, updated_at + touch trigger)` + partial index on due pending rows. Both tables are in `[api] exclude`; the CHECKs generate value-list enums in `db::types`, and `db::auth_tokens` / `db::outbound_mail` CRUD modules exist (features may not call another table's `write`; use the query files).
- **Security API (`src/shared/security/`):**
  - `password::{MIN_LENGTH = 8, MAX_LENGTH = 128}` (u64, for `#[validate(length(min = ..., max = ...))]`), `within_policy(&str) -> bool` (characters), `async hash(String) -> Result<String, AppError>`, `async verify(String, Option<String>) -> Result<bool, AppError>` (`None` = unknown email / no password: checked against `DUMMY_HASH`, always `false`). Both run in `actix_web::web::block` (actix's `spawn_blocking`); no tokio dependency was added.
  - `token::SecretToken::generate() -> { raw, hash }` (raw: 43-char base64url, mail it; hash: SHA-256 hex, store it) and `token::hash(&str)` to look a presented token up.
  - `jwt::JwtKeys::new(secret, ttl: Duration)`, `.issue(id: Uuid, role: Role, token_version: i32) -> Result<String, AppError>`, `.decode(&str) -> Result<Claims, AppError>` (401 on bad/expired; leeway 0), `.ttl()` for `TokenResponse.expires_in`. `Claims { sub: Uuid, role: Role, tv: i32, exp: u64 }`. `Role` is the generated `crate::db::types::Role` (no separate auth Role any more).
- **`AuthUser` / `AdminUser`** stay in `src/shared/auth.rs` (generated handlers import `crate::shared::auth::{AuthUser, AdminUser}`); it re-exports `Role`, `Claims` and `JwtKeys`. `AuthUser { id: Uuid, role: Role }` is still `Copy`. The extractor decodes the token with `web::Data<JwtKeys>`, then runs `session_state` on the **`web::Data<PgPool>` app data that `app::configure` already registers**: no account, inactive, or `tv != token_version` → 401; role from the DB. `AdminUser` → 403 for a non-admin.
- **Config (`src/config.rs`, documented in `.env.example`):** `Config { database_url, bind_addr, jwt_secret, jwt_ttl: Duration (JWT_TTL secs, 3600), require_verified_email: bool (true), app_url: String ("http://localhost:8080"), mail_transport: MailTransport::{Log, Smtp} (log), smtp_url: Option<String> (required when smtp), mail_from: String ("Actix API <no-reply@localhost>"), breach_check: bool (false), trust_proxy: bool (false), auth_rate_limit: u32 (20/min/IP), pool }`. `Config::from_vars(|name| ...)` reads through a lookup, for tests. Bools are `true|false|1|0`.
- **Wiring:** `app::configure(cfg, &PgPool, &web::Data<JwtKeys>)` is unchanged in shape; `run` builds `JwtKeys::new(&config.jwt_secret, config.jwt_ttl)`. Tickets that need more state (mailer, the auth settings) should extend `configure` and the test `call` together.
- **Template tests:** `tests/support/mod.rs` has `Database::fresh()` (fresh schema per test, skips without `DATABASE_URL`), `call(&pool, token, Method, uri, body) -> Answer { status, body }` (builds `App::new().configure(|cfg| configure(cfg, pool, &keys()))` per call), `keys()` (secret `"test secret"`, 1h), `token(id, role, tv)`, and `account(&pool, role) -> (Uuid, token)` which inserts a real user (`caller-<uuid>@test.invalid`) — tokens must now belong to a live row. `admin(&pool)` / `user(&pool)` wrap it. `tests/auth.rs` covers disabled/deleted/unknown accounts, a `token_version` bump and a role change. `tests/contract.rs` builds `JwtKeys::new("unused", Duration::from_secs(1))`.
- **Dependencies added:** `argon2 = "0.6"`, `base64 = "0.22"`, `rand = "0.10"`, `sha2 = "0.11"` (Cargo.lock gained only argon2, blake2, password-hash, phc). The template README's dependency block must match `Cargo.toml.template` (a test checks it).
