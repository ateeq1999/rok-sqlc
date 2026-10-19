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

**Status:** ready-for-agent

- [ ] Unit tests: hashing and verify, the policy, token hashing, JWT round trip, and a stale `tv` rejected.
- [ ] Integration (PostgreSQL): an inactive user's valid token → 401; a role change applies without re-login.
- [ ] `check-arch` rule tests. The template stays `check-arch` clean, clippy-clean on 1.94, and its contract test passes.
