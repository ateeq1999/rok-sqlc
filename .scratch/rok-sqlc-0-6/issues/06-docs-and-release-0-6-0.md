# 06: Docs, review fixes, and 0.6.0

**What to build:**
- **ADR 0018:** auth as hand-written template features (and why `[api.verifiable]` was dropped), the session model (access-only JWT plus `token_version`, a database-checked role), single-use hashed tokens, the mail outbox, the `400`/`422` split, rate limiting, and what's deferred (M6, not planned).
- **Guide:**
  - a new `docs/guide/auth.md`, covering each flow, the config variables, wiring a real SMTP server, and how to add a flow (a new token purpose plus its mail);
  - `wire-standard.md`: the new `400` codes and `429`;
  - `architecture.md`: query modules per feature, and the `check-arch` rule;
  - `troubleshooting.md`.
- **The template's own `README.md`/`AGENTS.md`:** the auth features.
- **0.6.0:**
  - bump the version everywhere (precedent: commit a191d97);
  - CHANGELOG `[0.6.0]`, breaking changes first: `AuthUser` now hits the database and needs the pool, the template's new migration, the `check-arch` rule;
  - README roadmap: the auth bullets move to done, except 2FA, uploads and API keys, which become M6;
  - `cargo publish --dry-run --locked`.

Do NOT tag or publish.

**Blocked by:** 01, 02, 03, 04, 05

**Status:** resolved

- [x] Drift tests, and the full suite on Rust 1.94 against Debian PostgreSQL.
- [x] The 1.88 build and the publish dry-run.

## Comments

**Implemented on `rok-sqlc-0-6-06`** (two commits: review fixes, then docs and release prep).

- **Review fixes (all done, test-first):**
  - Email change to a taken address now mails the old-address notice too (only the token and the new address's mail are skipped); test `a_change_to_a_taken_address_notifies_the_old_one_alone`.
  - A `dead` outbox row loses its text/HTML bodies (new query `RedactMail` in `queries/mail.sql`, called by the sender); test `a_dead_row_keeps_its_address_and_subject_but_not_its_link`. `token.rs` documents it.
  - `JWT_SECRET` under 32 bytes → `ConfigError::TooShort` (`MIN_JWT_SECRET_BYTES`); test/contract secrets lengthened; `.env.example` notes it.
  - Rule 4 reworded in `check_arch::rules` (so the scaffolded `AGENTS.md`), the architecture guide, the template's and `docs/examples/api`'s `AGENTS.md`. `SessionState` moved to `queries/session.sql` (`db::queries::session`).
  - Reset: new read-only query `LiveTokenPurpose` first (`400 invalid_token` before the breach check/argon2), hash outside the transaction, then consume + write. Test `a_bad_link_is_refused_before_the_new_password_is_checked`.
  - `jwt.rs` comment fixed (disable = per-request `is_active`); ADR 0018 records it.
  - `auth/service.rs`: `mail_link` (issue → render → enqueue, with `link_kind(purpose) -> (ttl, render)`) and `conflict_as`. **Deviation:** the commit stays at the call site (`self.outbox.commit(tx)`), because rule 2 forbids `service.rs` naming `sqlx::Transaction`.
- **Docs:** ADR 0018 (incl. the "leave as is" items: register 409, forgot timing, no lockout on change/email-change, `TRUST_PROXY` last hop, `TotpRequired`, sender holding its transaction); `docs/guide/auth.md` (added to `GUIDE_PAGES`, the index and the README); wire-standard (`400` flow codes, `429`, revoked-token `401`); architecture (query modules per feature); troubleshooting (real outputs: short `JWT_SECRET`, `SMTP_URL`, `cross-feature-query`, account/mail issues); template README (Accounts section, Coming next = M6) and AGENTS.md (Accounts section).
- **0.6.0:** version bumped as in a191d97; CHANGELOG `[0.6.0]` (breaking first); README roadmap. Adding a token purpose to a deployed DB hits rok-sqlc's unsupported `DROP CONSTRAINT` (documented in auth.md). Not tagged, published or pushed.
