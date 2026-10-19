# 04: Password forgot, reset and change

**What to build:** decisions Q5, Q7, Q14 and Q16.

- **`POST /auth/password/forgot` `{ email }`:** always answers `202 MessageResponse`. For a live account, it issues a `password_reset` token (1h; older reset tokens invalidated) and mails the link.
- **`POST /auth/password/reset` `{ token, new_password }`:**
  - Accepts a `password_reset` or an `invitation` token.
  - Checks the policy (`422`) and the breach check (`400 breached_password`).
  - Sets the hash, bumps `token_version`, clears `failed_logins`/`locked_until`, and uses up the token.
  - For an `invitation` token, also sets `email_verified = true`. (The invitation itself is ticket 05. Test this case by inserting an `invitation` token directly.)
  - A bad, expired or used token answers `400 invalid_token`.
- **`POST /auth/password/change` (user) `{ current_password, new_password }`:**
  - A wrong current password answers `400 wrong_password`; then the policy and breach checks apply.
  - Sets the hash, bumps `token_version` (the caller's own token stops working), and mails the password-changed notice.

**Blocked by:** 03. It can run in parallel with 05; coordinate merges in `features/auth/{routes,handlers,dto}.rs` by keeping additions local to new functions.

**Status:** resolved

- [x] Integration:
  - forgot for an unknown email → 202 with no mail;
  - forgot → reset → login with the new password; the old token is rejected (401), and the reset token can't be used again;
  - an expired token → 400;
  - change with a wrong current password → 400; change → the old JWT gets 401;
  - reset clears a lockout.
- [x] The contract covers these paths. Gates pass.

## Comments

**Implemented on `rok-sqlc-0-6-04`.** What 05 and later build on:

- **Routes** (in `auth::routes`, after `resend`): `POST /password/forgot` → `handlers::forgot` (`202`), `/password/reset` → `handlers::reset` (`200`), `/password/change` → `handlers::change` (`200`, `AuthUser`). All three are in `ApiDoc` and the contract's `COVERED`.
- **DTOs** (end of `auth/dto.rs`): `ForgotPasswordDto { email }` (normalized like `CredentialsDto`), `ResetPasswordDto { token (min 1), new_password (8–128) }`, `ChangePasswordDto { current_password, new_password (8–128) }`. Bad shapes are `422`.
- **Service** (a second `impl AuthService` block at the end of `auth/service.rs`, with `PASSWORD_RESET_TTL_HOURS = 1` above it): `forgot_password` (looks the live account up with `users::get_by_email`; unknown → nothing mailed), `reset_password` (breach check, then `consume_token` as `password_reset`, falling back to `invitation` on the same transaction; an invitation also verifies the email), `change_password(UserId, dto)` (`400 wrong_password` first, then the breach check; mails `templates.password_changed`). Feature dirs may only hold the standard files, so no `password.rs` split; the file is ~300 lines.
- **Queries** (end of `queries/auth.sql`): `reset_password(exec, id, Option<&str> hash, verify_email: bool) -> u64` (hash, `token_version + 1`, clears `failed_logins`/`locked_until`, `email_verified OR $3`; live accounts) and `change_password(exec, id, Option<&str>) -> u64` (hash, `token_version + 1`). A `0` from reset is `400 invalid_token` (the account was deleted).
- **Order of checks in reset:** a breached password is refused before the token is touched, so the link survives a `400 breached_password` or a `422`; an invalid token costs no argon2 hash.
- **Tests:** `tests/password.rs` (10): unknown-email forgot, forgot → reset → old JWT 401 / old password 401 / new 200 / reuse 400, expired and garbage tokens, a newer link replacing an older one, reset lifting a lockout, an invitation token (issued with `issue_token`) setting the first password and verifying the email, weak/breached on reset (the link still works after) and on change, wrong current password (the session survives), change → notice mailed and every session out. The template README describes the three endpoints.
