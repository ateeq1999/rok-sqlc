# 02: Two-factor authentication (both templates)

**What to build:** decision B2, in `templates/actix-api` and `templates/axum-api`.

- **Migration `0003_two_factor.sql`** (identical in both):
  - `users.totp_last_step BIGINT`;
  - `totp_recovery_codes(id, user_id → users ON DELETE CASCADE, code_hash, used_at, created_at)`;
  - `auth_tokens` purpose widened with `two_factor_pending`, through `DROP`/`ADD CONSTRAINT`, which ticket 01 teaches the parser.
  - `totp_secret` already exists; it now holds the AES-GCM ciphertext (base64 nonce+ciphertext).
- **Security (`shared/security/totp.rs`):** `totp-rs` and `aes-gcm`. Generate, encrypt and decrypt the secret, verify a code with ±1 step against the stored last step, generate and hash recovery codes. `TOTP_ENCRYPTION_KEY` (32 bytes, base64 or hex; say which) is checked in `config.rs` like `JWT_SECRET`.
- **Endpoints** (in `features/auth`, rate-limited, `#[utoipa::path]` per the contract):
  - `setup`: stores the encrypted secret with `totp_enabled` still false, and answers secret, URI and QR data URI.
  - `enable { code }`: on the right code, sets `totp_enabled`, stores 10 hashed recovery codes and answers them once. A wrong code, already-enabled 2FA or no setup answers `400`. Mails a notice.
  - `disable { current_password }`: on the right password, clears the secret, the flag and the codes; otherwise `400 wrong_password`. Mails a notice.
  - `verify { pending_token, code }`: a bad or expired token answers `400 invalid_token`. The code may be a TOTP code or an unused recovery code. A wrong code answers `401` and counts toward the lockout. Success spends the pending token and answers `TokenResponse`.
- **Login:** after the password (and the 403 checks) on a 2FA account, issue the 5-minute `two_factor_pending` token and answer `LoginResponse::TotpRequired`.
- `dto.rs`/`service.rs` stay byte-identical across the templates (drift test). The contract test covers the four paths.

**Blocked by:** 01

**Status:** resolved

- [x] PostgreSQL tests, in both templates:
  - setup → enable → login answers `requires_totp` → verify works;
  - the same code twice is refused (replay);
  - a recovery code works once;
  - wrong codes lock the account;
  - disable with the wrong password → 400;
  - an expired pending token → 400;
  - a login without 2FA is unchanged;
  - the secret is encrypted in the database.
- [x] Gates pass, including both templates' checks and the drift test.

## Comments

Done on `rok-sqlc-0-8-02`.

- **Migration `0003_two_factor.sql`** (identical in both): `users.totp_last_step BIGINT`, `totp_recovery_codes(id, user_id ON DELETE CASCADE, code_hash, used_at, created_at DEFAULT now())` + index on `user_id`, and ticket 01's `DROP`/`ADD CONSTRAINT auth_tokens_purpose_check` with `two_factor_pending` (`AuthTokenPurpose::TwoFactorPending`). `rok-sqlc.toml`: `totp_last_step` is managed and hidden, `totp_recovery_codes` is in `[api] exclude`. Checked-in `src/db/` regenerated.
- **`shared/security/totp.rs`** (byte-identical; drift test's `SHARED`): `TotpCipher::new(&[u8; 32])` with `seal(&[u8]) -> base64(nonce ‖ ciphertext+tag)`, `open(&str)`, `check(sealed, code, last_step) -> Option<step>`; `enroll(email) -> Enrollment { secret, base32, provisioning_uri, qr_code_data_uri }`; `matching_step(secret, code, now, last)` (±1 step, only steps after `last`, constant-time compare); `current_step()`, `is_code` (6 digits); `recovery_codes()` (10 × `XXXXX-XXXXX`, 32-char alphabet) and `recovery_hash` (SHA-256 of the trimmed, uppercased code). Issuer: `CARGO_PKG_NAME`. Unit tests use RFC 6238's SHA-1 vectors.
- **`TOTP_ENCRYPTION_KEY`** is required: standard base64 of exactly 32 bytes (`openssl rand -base64 32`), else `ConfigError::NotAKey`. `Config::totp_encryption_key: [u8; 32]`. `.env.example`, both READMEs, `docs/guide/auth.md`'s table, and `init --template`'s next step name it; `tests/support::TOTP_KEY` and `contract.rs` set it.
- **Wiring.** `AuthService.totp: Arc<TotpCipher>`; actix's `AppState` gains `totp` (built in `new`), axum builds it into `AppState.auth`.
- **Service** (a new `impl AuthService` block): `setup_totp` (400 when already on, via `StoreTotpSecret` updating only when off), `enable_totp` (`EnableTotp` also matches the secret the code was checked against, so a concurrent setup can't be enabled blind; replaces the codes; mails `Templates::two_factor_enabled`), `disable_totp` (wrong password → `400 wrong_password`; clears secret, flag, step and codes; mails `two_factor_disabled` when it was on), `verify_totp` (`PendingSignIn` peeks the token, so a wrong code doesn't burn it; locked → 401; disabled → 403; the code and the token are spent on one transaction; `AcceptTotpStep` is a conditional `UPDATE`, so two requests can't both use a step). `login` answers `LoginResponse` (handlers serialize it as is); with 2FA on it issues the pending token (`TWO_FACTOR_PENDING_TTL_MINUTES = 5`) and doesn't clear failed logins, so password logins can't reset the code lockout. `link_kind` became `ttl_of` + `link_mail` (`None` for `TwoFactorPending`). `FindLogin` returns `totp_enabled`.
- **Handlers** `setup_totp`, `enable_totp`, `disable_totp`, `verify_totp` (operation ids `setup`/`enable`/`disable`/`verify` as the contract has them), in `auth::routes` (rate-limited), `ApiDoc` and `COVERED`. DTOs `TotpSetupResponse`, `TotpEnabledResponse`, `EnableTotpDto`, `DisableTotpDto`, `VerifyTotpDto` (code ≤ 32 chars → 422).
- **Tests:** `tests/two_factor.rs` (10, same in both): the ticket's list plus a 5-minute TTL check and setup/enable preconditions; codes come from `totp-rs` on the base32 secret.
- **Deviations.** The QR code is an SVG data URI (`qrcode` with `svg` only), not a PNG, so no `image` dependency. `features/auth/service.rs` is ~680 lines: feature dirs allow only the standard files, so the 2FA flows are a fourth `impl` block in it (the crypto lives in `totp.rs`).
- **Dependencies:** `aes-gcm = 0.10` (`aes`, `alloc`), `qrcode = 0.14` (`svg`), `totp-rs = 5` (`otpauth`); both `Cargo.lock`s gained 16 packages (totp-rs 5.7.2, aes-gcm 0.10.3, qrcode 0.14.1 and their deps).
