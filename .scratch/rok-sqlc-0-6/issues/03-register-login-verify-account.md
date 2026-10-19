# 03: Register, login, email verification, the `account` feature, rate limiting

**What to build:** decisions Q7, Q9, Q12, Q13, Q14 (port only), Q17.

- **`features/auth/` (hand-written, generated layout):**
  - **`POST /auth/register`:**
    - The body is `CredentialsDto` (email format; password 8–128 characters → `422`).
    - Runs the breach check (the `BreachCheck` port in `shared/security/`: a HIBP range-API implementation plus a no-op one, chosen by `BREACH_CHECK`; fails open with a warning). A breached password answers `400 breached_password`.
    - A taken live email answers `409`.
    - Creates the user with role `user`, enqueues the verification mail on the same transaction, and answers `201 UserResponse`.
  - **`POST /auth/login`:**
    - Answers `200 TokenResponse { access_token, expires_in, token_type: "Bearer" }`.
    - Bad credentials, an unknown email (verified against the dummy hash) or a locked account → `401`. A disabled account, or an unverified email when `REQUIRE_VERIFIED_EMAIL` is on → `403`.
    - Lockout: 5 failures set `locked_until` 15 minutes ahead, and a success resets the counter.
    - The contract's `LoginResponse` `oneOf` includes the 2FA shape. Keep the schema, but M5 only ever returns the token.
  - **`POST /auth/email/verify` `{ token }`:** marks the email verified and uses up the token (`200 MessageResponse`). A bad, expired or used token answers `400 invalid_token`.
  - **`POST /auth/email/verification/resend` (user):** issues a new token (older ones invalidated) and mails it. Answers `202`, and also `202` with no mail when already verified.
- **`features/account/`:** `GET /auth/me`, `GET /users/me` and `PATCH /users/me` (`UpdateProfileDto { display_name }`), answering `UserResponse`. `app.rs` registers it before the generated features, so `/users/me` doesn't hit `/users/{id}`.
- **Errors:** add `400` codes `invalid_token`, `wrong_password` and `breached_password` to `AppError` and the wire-standard doc. Every handler carries `#[utoipa::path]` matching the contract plus our `422`.
- **Rate limit (Q9):** `actix-governor` on the `/auth` scope, per IP, with `AUTH_RATE_LIMIT` per minute. The IP comes from `X-Forwarded-For` only when `TRUST_PROXY=true`. An exceeded limit answers `429 too_many_requests` with the wire-standard body.

**Blocked by:** 01, 02

**Status:** ready-for-agent

- [ ] Integration (PostgreSQL, in-memory mailer):
  - register → mail captured → verify → login works;
  - login before verifying → 403 (and 200 with `REQUIRE_VERIFIED_EMAIL=false`);
  - duplicate email (case-insensitive) → 409;
  - a weak password → 422;
  - 5 bad logins → 401 even with the right password, until `locked_until` passes;
  - a used verify token → 400;
  - `/users/me` GET and PATCH;
  - the rate limit answers 429.
- [ ] The contract test covers these paths. Gates pass.
