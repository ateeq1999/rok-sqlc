# rok-sqlc 0.8: 2FA, API keys, media and avatars in both templates (milestone 6)

## Problem

Both templates leave 11 contract endpoints unimplemented: two-factor login, API keys, file uploads and profile photos. The login answer's `requires_totp` shape is declared but never used, and `avatar_url` is always missing.

## Decisions

The authority is `notes/m6-decisions.md` (B1–B7, settled 2026-10-21). See also ADR 0018 (auth) and ADR 0019 (axum).

## In scope

| Endpoint | Auth | Answers |
|---|---|---|
| `POST /auth/2fa/setup` | user | 200 `TotpSetupResponse` |
| `POST /auth/2fa/enable` | user | 200 `TotpEnabledResponse`, 400, 422 |
| `POST /auth/2fa/disable` | user | 200 `MessageResponse`, 400 `wrong_password` |
| `POST /auth/2fa/verify` | public | 200 `TokenResponse`, 400 `invalid_token`, 401 |
| `POST /auth/login` (changed) | public | also 200 `{ requires_totp, pending_token }` |
| `GET /api-keys` | user | 200 `[ApiKeyResponse]` |
| `POST /api-keys` | user | 201 `CreatedApiKeyResponse`, 409/422 (limit, shape) |
| `DELETE /api-keys/{id}` | user | 204, 404 |
| `GET /media` | user | 200 paginated `MediaResponse` |
| `POST /media` | user | 201 `MediaResponse`, 413, 415 |
| `GET /media/{id}` | user | 200, 404 |
| `DELETE /media/{id}` | user | 204, 404 |
| `GET /media/{id}/content` | user | 200 bytes, 404 |
| `PUT /users/me/avatar` | user | 200 `UserResponse`, 413, 422 |
| `DELETE /users/me/avatar` | user | 200 `UserResponse` |
| `GET /avatars/{file}` | public | 200 `image/jpeg`, 404 |

API keys also authenticate every `AuthUser` route (B4).

## Tickets

`issues/01`…`06`. Dependency graph: 01 → 02; 03 and 04 run alongside 01; 05 after 04; 06 after all.

## Out of scope

OAuth, passkeys and QR login (not planned).
