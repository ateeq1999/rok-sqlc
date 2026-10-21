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

**Status:** ready-for-agent

- [ ] PostgreSQL tests, in both templates:
  - setup → enable → login answers `requires_totp` → verify works;
  - the same code twice is refused (replay);
  - a recovery code works once;
  - wrong codes lock the account;
  - disable with the wrong password → 400;
  - an expired pending token → 400;
  - a login without 2FA is unchanged;
  - the secret is encrypted in the database.
- [ ] Gates pass, including both templates' checks and the drift test.
