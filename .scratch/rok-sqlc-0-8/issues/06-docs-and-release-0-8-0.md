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

**Status:** resolved

- [x] The full suite on Rust 1.94 against Debian PostgreSQL. The 1.88 build and the publish dry-run.

## Comments

Done on `rok-sqlc-0-8-06`, one commit per review fix, then docs and release prep.

**Review fixes (Part A).**
1. **actix API-key path bypass (fixed).** actix `AuthUser` checks `req.match_info().as_str()` (the decoded, routed path); the shared `check_reach` compares a `normalised` path (percent-decoded until stable, empty/`.`/`..` segments resolved, lower-cased). Unit test with encoded/unnormalised variants; `tests/api_keys.rs` (both): `api%2Dkeys`, `%61pi-keys`, `%61uth/me`, `au%74h/me` with a write key → 403/404, never 2xx (actix answered 200 before).
2. **Unknown `DROP CONSTRAINT` warns.** `ParsedSchema::warnings`, located `path:line: table `t`: …`, appended to `Report::warnings` (printed as `warning:`); no-op; `IF EXISTS` silent. Parser and CLI tests updated.
3. **`= ANY (ARRAY[…])`** is read (casts on column, array or values; varchar form included) only in `ALTER TABLE … ADD CONSTRAINT … CHECK`, rewritten to `IN (…)` before it is stored, so a later drop recomputes it. `CREATE TABLE` unchanged (a test pins that).
4. **`rename_constraint`:** unknown old → table failure, unless it ends `_not_null`/`_excl` (warning); taken new → failure. An old recovered test that renamed a missing constraint lost that line. `UniqueKey::counts` matches `is null` in any case/spacing. `clear_primary_key` shared by `alter.rs`/`named.rs`.
5. **Storage:** `AppState::new(pool, &config) -> Result<Self, object_store::Error>` opens `STORAGE_URL`; `support::config` and the contract's `served_document` set `memory://`. Test: `file://./target/test-storage-…` gets the bytes.
6. **Reset** revokes keys (`RevokeAllApiKeys`) and spends `two_factor_pending` tokens in its transaction; change keeps keys (documented). Tests in `password.rs`.
7. **Disable 2FA** bumps `token_version` (`DisableTotp`); old JWT → 401.
8. **Crypto:** recovery codes HMAC-SHA256 under `HMAC(TOTP_ENCRYPTION_KEY, label)`; AES-GCM AAD = account id; `subtle::ct_eq`; `Zeroizing` for the decrypted secret, enrollment secret and HMAC key. Deps `hmac 0.13`, `subtle 2`, `zeroize 1` (both `Cargo.lock`s: 3 lines each, no new packages). `TotpCipher::{recovery_codes, recovery_hash}` replace the free functions.
9. **actix body before auth: confirmed** (actix polls a handler's tuple of extractors together, so `web::Bytes` read the body while `AuthUser`'s DB lookup was pending). Handlers take `web::Payload`; `shared::body::read` (`to_bytes_limited`) reads it in the handler body; routes drop `PayloadConfig`, `media::routes(cfg)` lost its limit argument. Test: unauthenticated 9 MiB uploads (no token, unknown key) → 401 in both templates. "Not read" itself isn't observable in-process (no `Stream` impl without a new dev-dependency); axum already reads the body last.
10. **Line counts:** `auth/service.rs` 444, `two_factor/service.rs` 221, `shared/security/auth_token.rs` 113. The token lifecycle + `policy(purpose) -> (TimeDelta, Option<RenderLink>)` went to `auth_token.rs` (covered by the drift test's `src/shared/security` entry); still 616 lines, so 2FA became `features/two_factor/` (own `queries/two_factor.sql` with `RecordFailedCode`/`ClearFailedCodes`, its own `TokenResponse`/`MessageResponse` copies, routes under `/api/v1/auth/2fa` behind the same `RateLimit`, mounted before the auth scope; `HAND_WRITTEN` gained it). Lockout constants moved to `shared/security/password.rs`; `auth::service` re-exports them, the TTLs, `issue_token` and `consume_token`.
11. **413:** `AppError::too_large(what, limit)` in both `error.rs`; `shared/body.rs` per framework (axum `limited`, actix `read`).

**Docs/release (Part B).** ADR 0020; `docs/guide/files.md` (in `GUIDE_PAGES`); `auth.md` (2FA, API keys, settings); wire standard (413s, 415, API-key 401/403); troubleshooting with outputs captured from the actix binary and `rok-sqlc g`; READMEs, `AGENTS.md`, `rok-sqlc.toml`/`0001_init.sql` comments (schema.rs regenerated); 0.8.0 everywhere (Cargo.toml/lock, install scripts, release workflow, README); CHANGELOG `[0.8.0]`, breaking first; roadmap.

**Deviation:** the actix path fix isn't in the CHANGELOG's Fixed (API keys are new in 0.8.0).
