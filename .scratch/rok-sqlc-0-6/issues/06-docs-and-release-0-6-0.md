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

**Status:** ready-for-agent

- [ ] Drift tests, and the full suite on Rust 1.94 against Debian PostgreSQL.
- [ ] The 1.88 build and the publish dry-run.
