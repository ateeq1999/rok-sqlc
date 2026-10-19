# rok-sqlc 0.6: the auth surface in the actix template (milestone 5)

## Problem

`rok-sqlc init --template actix` creates an API whose users can't sign up, sign in, verify an email, or reset or change a password. It only checks a JWT that something else must have issued. The target contract (`templates/actix-api/tests/spec/openapi.json`, from the axum starter) defines these flows, and generated code can't express them: they need hashing, single-use tokens, mail delivery and rate limits.

## Decisions

The authority is `notes/m5-auth-decisions.md` (Q1–Q26, settled 2026-10-19). See also ADR 0016 (generated APIs), ADR 0017 (pre-checks) and the wire standard.

In short, auth is hand-written, user-owned features in the actix template. The generator gains nothing for auth. The one exception is a `check-arch` rule that lets a feature call its own query module.

## In scope (M5)

| Endpoint | Auth | Answers |
|---|---|---|
| `POST /auth/register` | public | 201 `UserResponse`, 400 `breached_password`, 409, 422 |
| `POST /auth/login` | public | 200 `TokenResponse`, 401, 403, 422 |
| `GET /auth/me` | user | 200 `UserResponse` |
| `POST /auth/email/verify` | public | 200 `MessageResponse`, 400 `invalid_token` |
| `POST /auth/email/verification/resend` | user | 202 |
| `POST /auth/email/change` | user | 202, 400 `wrong_password`, 422 |
| `POST /auth/email/change/confirm` | public | 200, 400 `invalid_token` |
| `POST /auth/password/forgot` | public | 202 |
| `POST /auth/password/reset` | public | 200, 400 `invalid_token`/`breached_password`, 422 |
| `POST /auth/password/change` | user | 200, 400 `wrong_password`/`breached_password`, 422 |
| `POST /auth/invitations` | admin | 201 `UserResponse`, 409, 422 |
| `GET /users/me` | user | 200 `UserResponse` |
| `PATCH /users/me` | user | 200 `UserResponse`, 422 |

Every path is under `/api/v1`. Schemas, operation ids and descriptions follow the contract document. Statuses are the contract's plus our documented `422` (Q13).

## Not in scope

The rest is planned as follows:
- **M6, for both templates:** 2FA, API keys, media and avatars.
- **Not planned:** OAuth, passkeys and QR login.
- **0.7:** the axum template.

## Tickets

`issues/01`…`06`. Dependency graph: 01 → 02 → 03 → (04 ∥ 05) → 06.
