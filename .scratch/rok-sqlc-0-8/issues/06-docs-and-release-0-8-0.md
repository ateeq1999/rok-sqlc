# 06: Docs, review fixes, and 0.8.0

**What to build:**
- **ADR 0020:** M6, covering B1–B7. Include the TOTP secret encryption, API-key rules (prefix, hash, scope, route limits), the storage port and the `object_store` choice, upload safety (type sniffing, limits, nosniff/attachment), avatar processing and the bomb limits, and the CHECK-constraint parser change.
- **Guide:**
  - `auth.md`: 2FA, API keys, the new config;
  - a new `docs/guide/files.md` covering storage, media, avatars, S3 setup and limits;
  - wire-standard: `413`, `415`, and API-key `403`s;
  - troubleshooting: real outputs, such as a bad `TOTP_ENCRYPTION_KEY` or `STORAGE_URL`.
- **Template READMEs and `AGENTS.md`.**
- **0.8.0:**
  - bump the version everywhere (precedent: commit a191d97);
  - CHANGELOG `[0.8.0]` with breaking changes first (new required `TOTP_ENCRYPTION_KEY`; `AuthUser` accepts API keys; the template migrations `0003`/`0004`);
  - README roadmap: M6 done; OAuth, passkeys and QR login not planned;
  - `cargo publish --dry-run --locked`.

Do NOT tag or publish.

**Blocked by:** 01, 02, 03, 04, 05

**Status:** ready-for-agent

- [ ] The full suite on Rust 1.94 against Debian PostgreSQL. The 1.88 build and the publish dry-run.
