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

**Status:** ready-for-agent

- [ ] Integration:
  - forgot for an unknown email → 202 with no mail;
  - forgot → reset → login with the new password; the old token is rejected (401), and the reset token can't be used again;
  - an expired token → 400;
  - change with a wrong current password → 400; change → the old JWT gets 401;
  - reset clears a lockout.
- [ ] The contract covers these paths. Gates pass.
