# Accounts and sign-in (the templates)

`rok-sqlc init --template actix` (or `--template axum`) makes an API whose
users sign up, verify their email, sign in (with a second factor if they
turn it on), reset or change their password, change their email, make API
keys for their scripts, and are invited by an admin. These flows are
hand-written features in the template, not generated code:
`src/features/auth/`, `two_factor/`, `api_keys/` and `account/` are
yours, like every `service.rs`. The decisions behind them are in ADR 0018
(`docs/adr/0018-auth-as-template-features.md`) and ADR 0020 (two-factor,
API keys, uploads; `docs/adr/0020-two-factor-api-keys-and-files.md`).
Uploads and profile photos have [their own page](files.md).
A plain `rok-sqlc init` has none of this, only the `AuthUser` and
`AdminUser` checks of a token something else issues.

The two templates hold the same flows: the services, DTOs, queries,
migrations, `shared/security/`, `shared/mail/` and the rate limit's
`limiter.rs` are byte-identical (a repository test checks it), and only
the handlers, the routes, the extractors and `app.rs` are written for
the framework (ADR 0019). Where this page shows actix-web wiring, the
axum template does the same in axum's terms.

- [Where the code is](#where-the-code-is)
- [The flows](#the-flows)
- [Two-factor sign-in](#two-factor-sign-in)
- [API keys](#api-keys)
- [Sessions](#sessions)
- [Settings](#settings)
- [Sending real mail](#sending-real-mail)
- [Adding a flow](#adding-a-flow)
- [Testing the flows](#testing-the-flows)

## Where the code is

| Path | What it holds |
|---|---|
| `src/features/auth/` | sign-up, sign-in, email verification, the password flows, email change, invitations |
| `src/features/two_factor/` | `/auth/2fa/setup`, `enable`, `disable` and `verify` |
| `src/features/api_keys/` | `GET` and `POST /api-keys`, `DELETE /api-keys/{id}` |
| `src/features/account/` | `GET /auth/me`, `GET` and `PATCH /users/me`, the profile photo |
| `src/shared/auth.rs` | the `AuthUser` and `AdminUser` extractors (`FromRequest` under actix-web, `FromRequestParts` under axum) |
| `src/shared/security/` | argon2id passwords, single-use tokens and their lifecycle (`auth_token.rs`), the JWT, the breach check, TOTP and its sealed secret (`totp.rs`), API keys (`api_key.rs`) |
| `src/shared/mail/` | the `Mailer` port, its SMTP and log transports, the messages, the outbox and its sender |
| `src/shared/rate_limit/` | the per-IP limit on `/auth`: `limiter.rs` counts, `mod.rs` is the framework's middleware |
| `queries/auth.sql` | the `auth` feature's SQL (`db::queries::auth`) |
| `queries/two_factor.sql`, `queries/api_keys.sql` | the `two_factor` and `api_keys` features' SQL |
| `queries/account.sql` | the `account` feature's SQL |
| `queries/session.sql`, `queries/mail.sql` | the session check and the outbox, for `shared/` |
| `migrations/0002_auth.sql` | `users.token_version`, `auth_tokens`, `outbound_mail` |
| `migrations/0003_two_factor.sql`, `0004_api_keys.sql` | `users.totp_last_step`, `totp_recovery_codes`, the `two_factor_pending` purpose; `api_keys`' indexes |

`check-arch` applies in full: `auth` writes through its own query module,
reads `users` through the data layer, and records audit entries through a
port onto the `audit_log` service ([architecture](architecture.md#4-the-six-rules)).

## The flows

Every path is under `/api/v1`, and every `/auth` path is rate-limited.
A body of the wrong shape (an email that isn't one, a password outside 8 to
128 characters) is a `422 validation_failed`; a request a flow refuses is a
`400` with its own code ([wire standard](wire-standard.md#errors)).

| Endpoint | Caller | Answers |
|---|---|---|
| `POST /auth/register` | anyone | `201` the user, `400 breached_password`, `409` |
| `POST /auth/login` | anyone | `200` a token, or `{requires_totp, pending_token}`; `401`, `403` |
| `POST /auth/2fa/setup` | user | `200` the secret, its URI and QR code; `400` |
| `POST /auth/2fa/enable` | user | `200` ten recovery codes; `400`, `422` |
| `POST /auth/2fa/disable` | user | `200`, `400 wrong_password` |
| `POST /auth/2fa/verify` | anyone | `200` a token, `400 invalid_token`, `401`, `403` |
| `GET /auth/me` | user | `200` the user |
| `POST /auth/email/verify` | anyone | `200`, `400 invalid_token` |
| `POST /auth/email/verification/resend` | user | `202` |
| `POST /auth/password/forgot` | anyone | `202` |
| `POST /auth/password/reset` | anyone | `200`, `400 invalid_token` or `breached_password` |
| `POST /auth/password/change` | user | `200`, `400 wrong_password` or `breached_password` |
| `POST /auth/email/change` | user | `202`, `400 wrong_password` |
| `POST /auth/email/change/confirm` | anyone | `200`, `400 invalid_token` |
| `POST /auth/invitations` | admin | `201` the user, `409` |
| `GET /users/me`, `PATCH /users/me` | user | `200` the user |
| `GET /api-keys`, `POST /api-keys` | user (session only) | `200` the active keys; `201` a new key once, `409`, `422` |
| `DELETE /api-keys/{id}` | user (session only) | `204`, `404` |

**Signing up.** `register` takes `{"email", "password"}`, creates a `user`
with an unverified email, and mails a verification link valid 24 hours. It
doesn't sign in. A live account with the same email, whatever its case, is
a `409`; a soft-deleted account's email can be registered again. The link
is `{APP_URL}/verify-email?token=…`: your front end serves that page and
posts the token to `POST /auth/email/verify` (`{"token"}`). A signed-in user
whose link expired asks for a new one with `resend`.

**Signing in.** `login` answers `{"access_token", "expires_in",
"token_type": "Bearer"}`. A wrong password, an unknown email or a locked
account is a `401` with one message for all. Five wrong passwords in a row
lock the account for 15 minutes. With the right password, a disabled
account, or an unverified email while `REQUIRE_VERIFIED_EMAIL=true`, is a
`403`. An account with two-factor on answers a pending token instead
([two-factor sign-in](#two-factor-sign-in)).

**A forgotten password.** `forgot` (`{"email"}`) always answers `202`, and
mails a link valid one hour (`{APP_URL}/reset-password?token=…`) when a live
account has that email. `reset` (`{"token", "new_password"}`) sets the
password, lifts any lock, and signs every older session out. It also
revokes every API key of the account and spends any pending two-factor
sign-in: a reset recovers an account someone else may have had. It checks the
link before anything else, so a bad link is a `400 invalid_token` that
costs no hash; a breached password is then a `400 breached_password`, and
the link still works.

**Changing a password.** `change` (`{"current_password", "new_password"}`)
needs the current one (`400 wrong_password`), signs every session out, the
caller's included, and mails a notice. API keys keep working: whoever
changes the password knew the old one; revoke a key you no longer trust.

**Changing an email.** `email/change` (`{"current_password", "new_email"}`)
needs the current password, then always answers `202`. The old address
gets a notice, and the new one a link valid one hour
(`{APP_URL}/confirm-email-change?token=…`), unless a live account already
uses it: then only the notice goes, so the answer and the old address's
mail don't say who has an account. `email/change/confirm` (`{"token"}`)
moves the account to the new address, verified; an address taken in the
meantime is a `400 invalid_token`.

**Invitations.** An admin posts `{"email", "display_name"?, "role"?}` to
`invitations`: the account is created with no password and an unverified
email, a `user.invited` audit entry is written, and a link valid 7 days
(`{APP_URL}/accept-invitation?token=…`) is mailed. The invitee posts that
token with a password to `POST /auth/password/reset`, which also verifies
the email.

**The account.** `GET /auth/me` and `GET /users/me` answer the signed-in
user; `PATCH /users/me` sets `display_name` (`null` clears it).

Change-password and email-change don't count toward the lockout: a stolen
session could otherwise lock the owner out. The `/auth` rate limit bounds
guessing there, as it bounds what `register`'s `409` and `forgot`'s timing
can tell about which emails exist.

## Two-factor sign-in

A signed-in user turns on a second factor from an authenticator app
(TOTP, RFC 6238: 6 digits, 30-second steps, one step of drift either way).

1. **`POST /auth/2fa/setup`** answers `{"secret", "qr_code_data_uri",
   "provisioning_uri"}`: a new secret in base32 (to type in), its
   `otpauth://` URI and that URI as an SVG QR code (a `data:` URI for an
   `<img src>`). Nothing is enforced yet; a new setup replaces an
   unconfirmed one. With two-factor already on it is a `400`.
2. **`POST /auth/2fa/enable`** (`{"code"}`) checks a code from the app
   against that secret and turns two-factor on. It answers
   `{"recovery_codes"}`, ten single-use `XXXXX-XXXXX` codes, shown this
   once, and mails a notice. A wrong code, or no setup, is a `400`.
3. From then on, **`POST /auth/login`** with the right password answers
   `{"requires_totp": true, "pending_token"}` instead of a session. The
   pending token lasts 5 minutes and is single-use.
4. **`POST /auth/2fa/verify`** (`{"pending_token", "code"}`) trades it,
   with a code from the app or an unused recovery code, for the session
   login would have answered. A bad, expired or used pending token is a
   `400 invalid_token`; a wrong code is a `401` and counts toward the
   lockout like a wrong password (5 in a row lock the account for 15
   minutes), and the pending token stays usable until it expires. A code's
   time step is spent when it is accepted, so the same code is never
   accepted twice, even by two requests at once.
5. **`POST /auth/2fa/disable`** (`{"current_password"}`) turns it off,
   forgets the secret and the recovery codes, mails a notice, and signs
   every session out, the caller's included (`400 wrong_password` for a
   wrong password). The contract asks only for the password; signing the
   sessions out is what stops a stolen session from keeping the account
   once two-factor is off.

**At rest.** The secret is stored in `users.totp_secret` sealed with
AES-256-GCM under `TOTP_ENCRYPTION_KEY`, with the account's id as
associated data, so a sealed secret copied onto another row doesn't open.
The recovery codes are stored as HMAC-SHA256 under a key derived from
`TOTP_ENCRYPTION_KEY`, never as themselves, so the table alone can't be
checked against guesses. `src/shared/security/totp.rs` holds all of it,
with no business meaning; the flows are `src/features/two_factor/`.

**Changing the key.** There is no rotation: a new `TOTP_ENCRYPTION_KEY`
can't open the secrets or match the recovery codes stored under the old
one, so every user with two-factor on would have to set it up again. Keep
the key with your other secrets, and back it up.

## API keys

A user makes keys for scripts and integrations: `POST /api-keys` with
`{"name", "scope"?, "expires_in_days"?}` answers `201` with the key,
`rok_<8 characters>_<32 characters>`, in `key`. It is shown this once;
only its first 12 characters (`key_prefix`, to recognise it) and its
SHA-256 are stored. `GET /api-keys` lists the caller's active keys
(neither revoked nor expired, newest first, with `last_used_at`), and
`DELETE /api-keys/{id}` revokes one (`204`; another user's is a `404`).

- **Sending one.** `Authorization: Bearer rok_…` (a bearer token starting
  `rok_` is a key, anything else a session JWT) or `X-API-Key: rok_…`.
  `Authorization` wins when both are sent.
- **What it reaches.** Every route `AuthUser` guards, acting as its owner,
  with the owner's role, an admin's included. `AuthUser::credential` says
  `Session` or `ApiKey(scope)`, for a handler that wants to tell. No key
  reaches `/api/v1/auth/*` or `/api/v1/api-keys` (`403 forbidden`): signing
  in, the account flows and managing keys need a session. The path is
  compared percent-decoded and normalised, so `/api/v1/api%2Dkeys` is
  refused like `/api/v1/api-keys`.
- **Scope.** `read` (the default) may only `GET`, `HEAD` and `OPTIONS`;
  any other method is a `403`. `write` may do what its owner may.
- **Lifetime.** `expires_in_days` is 1 to 365, or absent for a key that
  doesn't expire. An expired, revoked or unknown key, or a key whose owner
  is disabled or deleted, is a `401`. `last_used_at` is written at most
  once a minute.
- **Limits.** At most 25 active keys per user: the 26th is a `409
  conflict` until one is revoked. A name is 1 to 100 characters (`422`).
- **Recovery.** A password reset revokes every key of the account; a
  password change keeps them.

## Sessions

A session is an HS256 JWT signed with `JWT_SECRET`, carrying `sub` (the
user's id), `role`, `tv` (the account's `token_version`) and `exp`. There is
no refresh token: a client signs in again when it expires (`JWT_TTL`).

`AuthUser` checks the account on every request, with one query from
`queries/session.sql`: a deleted or disabled account, or a token whose `tv`
isn't the account's current `token_version`, is a `401`. A reset, a
password change or turning two-factor off bumps `token_version`, which
signs every older token out. An API key is checked the same way, against
its own row and its owner's (see [API keys](#api-keys)).
To disable an account, set `is_active = false`; its tokens stop working on
the next request. `AdminUser` reads the role from the database too, so a
promotion or demotion applies at once. So `AuthUser` needs the pool as app
data, `web::Data<PgPool>`, which `app::configure` registers (under axum,
`PgPool: FromRef<S>`, which `AppState` gives).

## Settings

`src/config.rs` reads them, and `.env.example` lists them:

| Variable | Default | Setting |
|---|---|---|
| `JWT_SECRET` | required | the HS256 key, at least 32 bytes (`openssl rand -base64 48`) |
| `JWT_TTL` | `3600` | seconds a session token lasts |
| `TOTP_ENCRYPTION_KEY` | required | the AES-256 key two-factor secrets are sealed under, and the recovery codes' HMAC key is derived from: 32 bytes in base64 (`openssl rand -base64 32`) |
| `REQUIRE_VERIFIED_EMAIL` | `true` | sign-in answers `403` until the email is verified |
| `APP_URL` | `http://localhost:8080` | the base of mailed links: your front end's URL |
| `MAIL_TRANSPORT` | `log` | `log` (each mail is logged) or `smtp` |
| `SMTP_URL` | required for `smtp` | the relay, such as `smtps://user:pass@host:465` |
| `MAIL_FROM` | `Actix API <no-reply@localhost>` | the sender of every mail |
| `BREACH_CHECK` | `false` | refuse passwords Have I Been Pwned knows |
| `TRUST_PROXY` | `false` | take the client IP from `X-Forwarded-For` |
| `AUTH_RATE_LIMIT` | `20` | requests a minute per IP on `/auth`; `0` turns it off |

The upload settings (`STORAGE_URL`, `MEDIA_MAX_UPLOAD_BYTES`,
`MEDIA_ALLOWED_TYPES`) are on [Files and storage](files.md#settings).

The defaults suit development: mail is logged (`RUST_LOG=debug` shows
each body, links included), the breach check is off, and email
verification is required.

**The breach check** sends Have I Been Pwned's range API the first five hex
characters of the password's SHA-1, never the password. If the service
can't be reached, the password is accepted and a warning logged.

**The rate limit** counts per client IP in the process: several instances
each allow `AUTH_RATE_LIMIT`. Over the limit, a request answers `429
too_many_requests` with `Retry-After`. Without `TRUST_PROXY` the IP is the
TCP peer's. With `TRUST_PROXY=true` it is the last address of
`X-Forwarded-For`, the one the proxy in front of the API appended. That is
right behind one proxy you run. Behind a CDN and a load balancer, the last
address is the CDN's: make the last proxy put the client's IP last, or set
`AUTH_RATE_LIMIT=0` and limit at the edge. Never set `TRUST_PROXY` when
clients reach the API directly: they could send any address.

## Sending real mail

Set `MAIL_TRANSPORT=smtp`, `SMTP_URL` and `MAIL_FROM`:

```sh
MAIL_TRANSPORT=smtp
SMTP_URL=smtps://apikey:SECRET@smtp.example.com:465      # TLS from the start
# SMTP_URL=smtp://apikey:SECRET@smtp.example.com:587?tls=required   # STARTTLS
MAIL_FROM="My App <no-reply@example.com>"
APP_URL=https://app.example.com
```

Any relay that speaks SMTP works (your provider's, or a local one such as
Mailpit at `smtp://localhost:1025` in development). The sender domain needs
SPF and DKIM records for the mail to reach inboxes; your provider documents
them.

Mail leaves through an outbox. A flow writes its message as an
`outbound_mail` row on its own transaction, so a flow that fails sends
nothing, and a task started with the server sends it right after the
commit, and every 5 seconds whatever is due. A failed send is retried after
30 seconds, then 1, 2 and 4 minutes; after the fifth failure the row is
`dead`: it keeps its address, subject and `last_error`, loses its bodies
(and the link in them), and is never tried again. Look for them with:

```sql
SELECT id, payload->>'to' AS "to", payload->>'subject' AS subject, last_error, updated_at
FROM outbound_mail WHERE status = 'dead' ORDER BY updated_at DESC;
```

A sent row is deleted. Delivery is at least once: a crash between sending a
mail and recording it sends it again. Several instances share the outbox
safely; each claims rows the others aren't sending. `/health/ready` doesn't
depend on mail.

## Adding a flow

A flow that mails a link needs a token purpose, a lifetime, a message and
an endpoint. Say a sign-in link, `magic_link`.

**The purpose.** Add `'magic_link'` to the `purpose` check of `auth_tokens`
and run `rok-sqlc g`: `AuthTokenPurpose` gains `MagicLink`. Before your
first deployment, edit the list in `migrations/0002_auth.sql`. On a
database that already ran it, write a new migration that drops the check
by its name and adds the wider one; rok-sqlc follows both:

```sql
ALTER TABLE auth_tokens DROP CONSTRAINT auth_tokens_purpose_check;
ALTER TABLE auth_tokens ADD CONSTRAINT auth_tokens_purpose_check CHECK (
    purpose IN ('email_verification', 'password_reset', 'email_change',
                'invitation', 'magic_link')
);
```

The inline check's name is the one PostgreSQL gave it,
`auth_tokens_purpose_check`.

**The message.** Add a method to `Templates` in
`src/shared/mail/templates.rs`, like `verification`, with a path constant
for the page your front end serves. `compose` escapes the HTML body, so
values go in as they are:

<!-- source: templates/actix-api/src/shared/mail/templates.rs -->
```rust
    pub fn verification(&self, to: &str, token: &str) -> Mail {
        compose(
            to,
            "Verify your email address",
            &[
                "Confirm that this is your email address to activate your account. The link is valid for 24 hours.",
                "If you didn't create an account, ignore this email.",
            ],
            Some(Action {
                label: "Verify email",
                url: self.link(VERIFY_EMAIL_PATH, token),
            }),
        )
    }
```

**The lifetime.** Add a `MAGIC_LINK_TTL_MINUTES` beside the others in
`src/shared/security/auth_token.rs`, and the arm the compiler then asks
for in `policy`: each purpose's lifetime and the message that carries it
(`None` for a token that is never mailed):

<!-- source: templates/actix-api/src/shared/security/auth_token.rs -->
```rust
pub fn policy(purpose: AuthTokenPurpose) -> (TimeDelta, Option<RenderLink>) {
    match purpose {
        AuthTokenPurpose::EmailVerification => (
            TimeDelta::hours(VERIFICATION_TTL_HOURS),
            Some(Templates::verification),
        ),
```

**The service methods.** One mails the link on a transaction:
`let mut tx = self.pool.begin().await?;`, then
`self.mail_link(&mut tx, user.id, &user.email, AuthTokenPurpose::MagicLink, None).await?;`
(it issues the token, spending the user's older ones, and enqueues the
message), then `self.outbox.commit(tx).await`. The other redeems it:
`consume_token(&mut tx, raw, AuthTokenPurpose::MagicLink)` spends the
token or answers `400 invalid_token`. Any SQL goes in `queries/auth.sql`.

**The endpoints.** A handler per method in `handlers.rs` (parse, call,
answer), a `.route(...)` line in the `/api/v1/auth` scope of `routes.rs`
(under axum, the router `routes.rs` nests under `/api/v1/auth`), so the
rate limit covers it, a DTO in `dto.rs`, and the handler in
`ApiDoc`'s `paths(...)` in `src/openapi.rs`. Then drive it end to end, as
below.

## Testing the flows

`tests/` drives each flow over HTTP against PostgreSQL (set
`DATABASE_URL`; each test makes and drops its own schema). The helpers in
`tests/support/mod.rs`:

- `call(&pool, token, method, uri, body)` sends a request to the whole
  application; `call_with(&state, …)` shares one `AppState`, and so one
  rate limit, across calls. Swap a port on the state: with actix,
  `state.breach = Arc::new(MyCheck)`; with axum, whose `AppState` builds
  each service once, `state.with_breach(Arc::new(MyCheck))` (or
  `with_audit`).
- `mailed(&pool)` delivers the outbox through an in-memory mailer and
  answers what was sent; `mailed_token(&mail)` reads the token of the link
  a mail carries.
- `register_verified(&pool, email, password)` signs up, verifies and signs
  in, answering a session token.

`tests/sign_up.rs`, `login.rs`, `password.rs`, `email_change.rs`,
`invitations.rs` and `mail.rs` are worked examples.
