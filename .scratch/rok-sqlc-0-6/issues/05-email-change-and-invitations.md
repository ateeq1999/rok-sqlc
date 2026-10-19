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

**Status:** resolved

- [x] Integration:
  - change → confirm → login with the new email (and not the old);
  - change to a taken address → 202 with no mail to it;
  - wrong password → 400;
  - confirm after the address was taken in between → 400;
  - invite → audit row exists in the same commit → `password/reset` with the invitation token → login works, and the email is verified;
  - a non-admin invite → 403;
  - a failed audit write rolls back the user.
- [x] The contract covers these paths. `check-arch` passes, with the port. Gates pass.

## Comments

**Implemented on `rok-sqlc-0-6-05`.**

- **Endpoints** (in `auth`'s scope, rate-limited): `POST /auth/email/change` (`AuthUser`, `ChangeEmailDto`, `202`), `POST /auth/email/change/confirm` (`VerifyEmailDto`, `200`), `POST /auth/invitations` (`AdminUser`, `InviteUserDto { email, display_name?, role? }`, `201 UserResponse`). Handlers `request_change`, `confirm_change`, `invite`; in `ApiDoc` and the contract's `COVERED` (one new `KNOWN` `avatar_url` line for the invite's `201`).
- **Service** (new `impl AuthService` blocks at the end of `auth/service.rs`): `request_email_change`, `confirm_email_change`, `invite`; TTLs `EMAIL_CHANGE_TTL_HOURS = 1`, `INVITATION_TTL_DAYS = 7`. The taken-address check on request reuses `find_login` (case-insensitive, live accounts); a taken address mails nothing at all, not even the notice. Confirm doesn't pre-check: the unique violation of `users_email_key` (taken meanwhile, or a race) maps to `400 invalid_token` and rolls the transaction back (the token stays unspent).
- **Queries** (`queries/auth.sql`, at the end): `ChangeEmail :execrows` (sets `email`, `email_verified = true`), `InviteUser :one` → `invite_user(exec, InviteUserParams { id, email, display_name, role })`.
- **The port (ADR 0016):** `auth/service.rs` declares `#[async_trait] trait AuditTrail { async fn record_in(&self, &mut PgConnection, AuditEntry { actor, target, action }) }`; `AuthService.audit: Arc<dyn AuditTrail>`. `app.rs` implements it as `pub struct AuditLog(pub AuditLogEntryService)`, calling `AuditLogEntryService::create_in` on the flow's connection. **Deviation:** `audit_log` skips `create` in `rok-sqlc.toml`, so the generated trait has no `create_in`; it is an inherent method in the scaffolded `audit_log_entry/service.rs` (over `repo::create`). `AppState.audit` (built in `AppState::new`) lets a test swap it.
- **Tests:** `tests/email_change.rs` (5) and `tests/invitations.rs` (3). The rollback test wraps the real `AuditLog` port and fails after its write, so it proves the entry, the user and the mail share one transaction. After merging ticket 04, the invitation test goes invite → mailed token → `POST /auth/password/reset` → login → `GET /auth/me` (`email_verified`, `has_password` true).
- **Merge with 04:** both `impl AuthService` blocks kept (04's password flows, then email change, then the port and invitations); queries, routes, `ApiDoc` paths and `COVERED` keep 04's entries first. `auth/service.rs` is ~450 lines.
- Docs: README's stale "email changes through the flow to come" line and the matching `rok-sqlc.toml` comment; the rest is ticket 06's.
