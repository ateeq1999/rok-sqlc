# 05: Email change and invitations

**What to build:** decisions Q11, Q15 and Q18.

- **`POST /auth/email/change` (user) `{ current_password, new_email }`:**
  - A wrong password answers `400 wrong_password`; an invalid email format, `422`.
  - Otherwise it always answers `202`, even when `new_email` is taken by another live account (then no token and no mail to the new address).
  - When the address is free, it issues an `email_change` token (1h, `new_email` stored; older ones invalidated), mails it to the new address, and mails a notice to the old one.
- **`POST /auth/email/change/confirm` `{ token }`:**
  - If the new email is still free, sets `email` to it and `email_verified = true`, and uses up the token. Answers `200 MessageResponse`.
  - A taken address, or a bad, expired or used token, answers `400 invalid_token`.
  - A race against the unique index also answers `400 invalid_token`, not `409`.
- **`POST /auth/invitations` (admin) `InviteUserDto { email, display_name?, role }`:**
  - Creates the user with no password and `email_verified = false`, and issues an `invitation` token (7d) with the invitation mail.
  - Writes an `audit_log` entry (action `user.invited`, actor = admin, target = new user) through a **port** onto the `audit_log` feature's `create_in`, all on one transaction. This is the template's first port: implement it in `app.rs` as ADR 0016 describes.
  - Answers `201 UserResponse`; a taken email, `409`.

**Blocked by:** 03. It can run in parallel with 04 (see 04's note).

**Status:** ready-for-agent

- [ ] Integration:
  - change → confirm → login with the new email (and not the old);
  - change to a taken address → 202 with no mail to it;
  - wrong password → 400;
  - confirm after the address was taken in between → 400;
  - invite → audit row exists in the same commit → `password/reset` with the invitation token → login works, and the email is verified;
  - a non-admin invite → 403;
  - a failed audit write rolls back the user.
- [ ] The contract covers these paths. `check-arch` passes, with the port. Gates pass.
