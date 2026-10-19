# Milestone 5: auth surface decisions (Q1–Q26)

Settled with the maintainer on 2026-10-19 in a design session. This note is the authority for `../spec.md`. It replaces Q55's `[api.verifiable]` idea (see Q1).

## Scope and placement

- **Q1:** Auth is hand-written features in `templates/actix-api/`, later in the axum template, ported from the starter (`axum-api-starter-v2`) and reshaped to the feature layout, ports and `check-arch`. The generator emits no auth. `[api.verifiable]` is dropped: email change and password change are template flows. A plain `rok-sqlc init` keeps only the `AuthUser`/`AdminUser` JWT check. `[api.immutable]` keeps `email` out of the generated update.
- **Q2:** M5 (0.6.0) covers register, login with lockout, `GET /auth/me`, password forgot/reset/change, email verify/resend, email change/confirm, invitations, and the mail outbox with a mailer. M6 covers 2FA (TOTP and login's `requires_totp` + `pending_token`), API keys (with an `ApiKey` extractor), and media and avatars (with a storage port). OAuth, passkeys and QR login are not planned. The contract test covers each feature's paths as it lands.
- **Q26:** After M5 comes the axum template (0.7), with the generated features and M5's auth. M6 follows, built once for both templates.

## Session and credentials

- **Q3:** Sessions are an access-only HS256 JWT with `sub`, `role`, `tv` and `exp`. `JWT_TTL` defaults to 1h. There's no refresh token, matching the contract's `TokenResponse { access_token, expires_in, token_type }`. A password change, a reset or an admin disable bumps `users.token_version`, and a token whose `tv` doesn't match is rejected.
- **Q21:** `AuthUser`'s per-request lookup loads `token_version`, `is_active` and `role` together. An account that's disabled or soft-deleted gets `401`. `AdminUser` checks the role from the database, not from the token, so a promotion or demotion applies on the next request.
- **Q4:** Passwords are hashed with argon2id (the `argon2` crate, recommended parameters) inside `spawn_blocking`. The policy is 8–128 characters with no composition rules (NIST 800-63B). Login with an unknown email still verifies against a dummy hash, so timing doesn't reveal which emails exist.
- **Q14:** A `BreachCheck` port in `shared/`, implemented with the Have I Been Pwned range API (k-anonymity: only 5 hex characters of the SHA-1 are sent). It's off by default (`BREACH_CHECK=false`), fails open with a warning when the service is unreachable, and applies to register, reset and change.

## Single-use tokens

- **Q5:** One `auth_tokens` table with `purpose` ∈ `email_verification`, `password_reset`, `email_change` (with `new_email`) and `invitation`.
  - Only the SHA-256 of a 32-byte random token is stored.
  - A token is single-use (`used_at`). Issuing a new one for the same user and purpose invalidates the older ones.
  - Lifetimes: verification 24h, reset 1h, email change 1h, invitation 7d.
  - The token is delivered as a link in an email; the confirm endpoints take `{ token }`.

## Mail

- **Q6:** A `Mailer` port in `shared/`, with an SMTP transport (`lettre`, rustls) and a `log` transport, chosen by config.
  - An `outbound_mail` outbox row is written in the same transaction as the flow's token. Templates are text plus HTML.
  - No general jobs queue.
  - A test-only in-memory mailer captures the tokens (Q20).
- **Q24:** `main.rs` starts one tokio sender task.
  - It wakes every 5 seconds, or immediately when a flow writes a mail row.
  - It claims rows with `FOR UPDATE SKIP LOCKED`, so several instances are safe.
  - It backs off between attempts and marks a row `dead` after 5 failures.
  - It stops when the server shuts down.
  - `/health/ready` doesn't depend on mail.

## Flows and answers

- **Q7:** `forgot`, `resend` and `email/change` (when the password is right) always answer `202`. `register` answers `409` for a taken email (the contract's choice), and the rate limit caps how much that can be abused. `login` answers:
  - `401` for bad credentials or a locked account;
  - `403` for a disabled account, or an unverified email when `REQUIRE_VERIFIED_EMAIL` is on (default `true`).

  Lockout: 5 failures lock the account for 15 minutes (`failed_logins`, `locked_until`).
- **Q13:** Failures split by kind:
  - **Shape problems** answer `422 validation_failed` with field details (wire standard), such as an invalid email or a password outside 8–128 characters.
  - **Flow failures** answer `400` with specific codes: `invalid_token` (bad, expired or used link), `wrong_password` (current password), `breached_password`.

  The documents keep the contract's `400`s and add our `422`.
- **Q12:** Register answers `201 UserResponse`, with no automatic login. The verification mail goes through the outbox in the same transaction. A soft-deleted account's email can be registered again.
- **Q11:** Invitation (admin) creates the user with no password, the given role and `email_verified = false`, and issues an `invitation` token. It answers `201 UserResponse`, and a taken email is `409`. The invitee calls `POST /auth/password/reset` with that token, which sets the password and also sets `email_verified = true`.
- **Q15:** Email change `{ current_password, new_email }`:
  - A wrong password answers `400 wrong_password`.
  - Otherwise the answer is always `202`, even when `new_email` is taken.
  - It mails a token to the new address and a notice to the old one.

  On confirm, the email must still be free (otherwise `400 invalid_token`). Confirm sets `email` and `email_verified = true` and uses up the token.
- **Q16:**
  - **Change:** checks the current password, stores the new hash, bumps `token_version` (signing out every token, the current one included) and mails a notice.
  - **Reset:** sets the hash, bumps `token_version` and clears the lockout.
- **Q17:** A hand-written `account` feature owns `GET /auth/me`, `GET /users/me` and `PATCH /users/me` (`UpdateProfileDto`: `display_name`). `app.rs` registers it before the generated features, so `/users/{id}` doesn't capture `/users/me`.
- **Q18:** Invitations (and later admin disables) write an `audit_log` row through a port onto the `audit_log` service's `create_in`, on the same transaction.
- **Q9:** An in-process per-IP rate limiter (`actix-governor`) on the `/auth` scope, with the limit in `Config`. `X-Forwarded-For` is trusted only when `TRUST_PROXY=true`.

## Code placement

- **Q8:** Auth's SQL goes in the template's query files (`queries/auth.sql`, and per feature), which rok-sqlc generates into `db/queries/<stem>.rs`. Auth calls those instead of another feature's repository.
- **Q22:** `check-arch`: a feature may call `db::queries::<its own feature name>`. Calling another feature's query module is reported, like `db::<table>::write`.
- **Q19:** Hand-written features use the generated features' folder layout: `features/auth/{mod,routes,handlers,dto,service}.rs` and `features/account/`. They're user-owned, and `check-arch` applies in full. Mail goes in `shared/mail/`; hashing and tokens go in `shared/security/`.
- **Q10:** Template migration `0002_auth.sql` adds `users.token_version`, `auth_tokens` and `outbound_mail`. `token_version` is added to `managed_columns`, and `auth_tokens` and `outbound_mail` to `[api] exclude`.
- **Q23:** Config, read only in `config.rs` and documented in `.env.example`: `JWT_TTL`, `REQUIRE_VERIFIED_EMAIL`, `APP_URL` (link base, such as `{APP_URL}/verify-email?token=…`), `MAIL_TRANSPORT` (`smtp`|`log`), `SMTP_URL`, `MAIL_FROM`, `BREACH_CHECK`, `TRUST_PROXY` and `AUTH_RATE_LIMIT` (per minute per IP). The defaults suit development: the `log` transport, breach check off, verified email required.

## Testing and release

- **Q20:** The contract test covers every M5 path. PostgreSQL integration tests drive each flow end to end with the in-memory mailer:
  - register → verify → login;
  - forgot → reset → the old token is rejected;
  - email change → confirm;
  - lockout;
  - invitation → reset.
- **Q25:** Ships as **0.6.0**: the template is embedded in the binary, so `init --template actix` output changes. Docs: ADR 0018 (auth as template features, the session model, tokens, the outbox, the `400`/`422` split), a new `docs/guide/auth.md`, and the README roadmap. Workflow: integration branch `rok-sqlc-0-6`, review, a PR to `main`, and the maintainer pushes the tag.
