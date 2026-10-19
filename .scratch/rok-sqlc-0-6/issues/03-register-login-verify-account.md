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

**Status:** resolved

- [x] Integration (PostgreSQL, in-memory mailer):
  - register → mail captured → verify → login works;
  - login before verifying → 403 (and 200 with `REQUIRE_VERIFIED_EMAIL=false`);
  - duplicate email (case-insensitive) → 409;
  - a weak password → 422;
  - 5 bad logins → 401 even with the right password, until `locked_until` passes;
  - a used verify token → 400;
  - `/users/me` GET and PATCH;
  - the rate limit answers 429.
- [x] The contract test covers these paths. Gates pass.

## Comments

**Implemented on `rok-sqlc-0-6-03`.** What tickets 04–05 build on:

- **Deviation (Q9): no `actix-governor`.** It is GPL-3.0-or-later, which a template users ship can't carry. `src/shared/rate_limit.rs` is a small middleware over `governor` (MIT, `default-features = false, features = ["std"]`): `RateLimit::per_minute(n, trust_proxy)` (`0` = off), an actix `Transform`, so `.wrap(limit.clone())` on a scope or resource. Every clone shares one keyed GCRA limiter, built once in `AppState::new` (`state.rate_limit`). The IP is the peer, or with `TRUST_PROXY=true` the **last** `X-Forwarded-For` address (the one your proxy appended); no address → `0.0.0.0`. Over the limit it answers `429 too_many_requests` with `Retry-After` itself (an `Err` from middleware panics `test::call_service`).
- **Layout.** `features/auth/{mod,routes,handlers,dto,service}.rs` and `features/account/` (same five files), headers `Scaffolded once`, declared in `features/mod.rs`. `auth::routes(cfg, &RateLimit)` is one `web::scope("/api/v1/auth").wrap(limit)` with one `.route(...)` per line: add yours there. `account::routes` mounts `/api/v1/auth/me` (a resource with the same limit) and `/api/v1/users/me`; `app::configure` mounts `account`, then `auth`, then `configure_all`. Add each new handler to `ApiDoc`'s `paths(...)` in `openapi.rs` (one per line), and the contract's `COVERED` (plus a `KNOWN` `avatar_url` line for any `UserResponse` body).
- **`AuthService`** (`service.rs`, pub fields, built in `app::configure`): `pool`, `keys: Arc<JwtKeys>`, `outbox: Arc<Outbox>`, `templates: Arc<Templates>`, `breach: Arc<dyn BreachCheck>`, `require_verified_email`. Methods `register`, `login`, `verify_email`, `resend_verification`, and `reject_breached(&str) -> Result<(), AppError>` (the `400 breached_password`; call it before hashing in reset/change). Add a field here and in `configure` for new deps (an audit port for invitations).
- **Token helpers** (free functions in `auth/service.rs`): `issue_token(&mut PgConnection, UserId, AuthTokenPurpose, chrono::TimeDelta, new_email: Option<String>) -> Result<SecretToken, AppError>` revokes the unused tokens of that user+purpose, then stores the new hash; mail `secret.raw`. `consume_token(&mut PgConnection, raw, purpose) -> Result<ConsumeTokenRow { user_id, new_email }, AppError>` spends it in one `UPDATE … RETURNING` (unused, unexpired, right purpose) or answers `400 invalid_token`. Pass `&mut tx` (a `pool.begin()` transaction); commit with `outbox.commit(tx)` when mail was enqueued. `VERIFICATION_TTL_HOURS = 24`; add your TTLs beside it.
- **Queries (`queries/auth.sql` → `db::queries::auth`):** `create_user(exec, UserId, &str, Option<&str>) -> User` (`RETURNING *`; role `user`), `record_failed_login(exec, id, max, minutes)` (at the max: lock and reset the count), `clear_failed_logins(exec, id)`, `mark_email_verified(exec, id) -> u64`, `revoke_tokens(exec, user_id, purpose)`, `insert_token(exec, InsertTokenParams)`, `consume_token(exec, &hash, purpose) -> Option<ConsumeTokenRow>`. Reset needs its own `set password_hash, bump token_version, clear lockout` query; invitations a create with role and no password. `queries/account.sql`: `update_display_name`. Users may be *read* through `db::users::read` (`get_by_id`); never call `db::users::write` from `auth`.
- **Errors (`error.rs`):** `AppError::InvalidToken(String)`, `WrongPassword(String)`, `BreachedPassword(String)` → `400` with codes `invalid_token`/`wrong_password`/`breached_password`; `TooManyRequests { retry_after_secs }` → `429`. Documented in `docs/guide/wire-standard.md`. `WrongPassword` is unused yet (04/05 answer it from `password::verify(current, Some(hash))` being `false`).
- **DTOs (`auth/dto.rs`):** `CredentialsDto` (email trimmed+lowercased by the private `normalized_email` deserializer, reuse it for `ForgotPasswordDto`/`ChangeEmailDto`/`InviteUserDto`; password `length(min = MIN_LENGTH, max = MAX_LENGTH)`), `VerifyEmailDto { token }` (reuse for email-change confirm), `TokenResponse`, `LoginResponse` (untagged `Session(TokenResponse)` | `TotpRequired { requires_totp, pending_token }`; only `Session` is answered), `MessageResponse::new(&str)`, and a `UserResponse` (each feature has its own copy; check-arch forbids importing `users`'s).
- **BreachCheck** (`shared/security/breach.rs`): `#[async_trait] trait BreachCheck { async fn is_breached(&self, &str) -> bool }`, `NoBreachCheck`, `PwnedPasswords::new(base_url)` (HIBP range API via `reqwest` 0.12 rustls/ring, `Add-Padding`, 3 s timeout, fails open with a `warn`), `from_config(&Config)`. `AppState.breach: Arc<dyn BreachCheck>`.
- **AppState** gained `breach`, `rate_limit`, `require_verified_email`. Tests no longer build it field by field: `support::config(&[overrides]) -> Config`, `state(pool)?` / `state_with(pool, &[("REQUIRE_VERIFIED_EMAIL", "false")])?` call `AppState::new`; `contract.rs` does too. So a new `AppState` field only needs `AppState::new`.
- **Test helpers (`tests/support`):** `call_with(&state, token, method, uri, body)` (calls sharing a state share its rate limit; `call` builds a fresh state per call), `PASSWORD`, `register_verified(pool, email, password) -> access token` (register, verify via the mailed link, sign in; drains the outbox). Swap in a fake breach check with `state.breach = Arc::new(MyCheck)` (see `tests/sign_up.rs`'s `KnowsOne`). Test files: `tests/sign_up.rs`, `tests/login.rs`, `tests/account.rs`.
- **Login:** `401` (one message for all: wrong password, unknown email, locked) is decided before `403` (disabled, unverified), so a `403` needs the right password. `MAX_FAILED_LOGINS = 5`, `LOCK_MINUTES = 15`; a lock resets the count, a success clears both. A reset (04) should clear the lockout too.
- **Dependencies added:** `governor = { version = "0.10", default-features = false, features = ["std"] }`, `reqwest = { version = "0.12", default-features = false, features = ["rustls-tls"] }`, `sha1 = "0.11"`. README block matches.
