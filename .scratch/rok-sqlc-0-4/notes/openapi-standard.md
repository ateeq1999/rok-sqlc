# OpenAPI standard extracted from api-starter-axum

Source: https://3-216-222-160.sslip.io/api-docs/openapi.json (raw copy: openapi.json, fetched 2026-10-18).
"Live" below means the running server was probed with read-only or failing requests (no accounts created).

## 1. Info
- title `api-starter-axum`, version `0.1.0`, OpenAPI `3.1.0` (utoipa-style output from Rust/axum).
- No `servers` block and no global `security`. Security is set per operation.
- One scheme: `bearer_auth` (http bearer, format JWT). The token is a session JWT from `/auth/login` or `/auth/2fa/verify`, or an API key `ak_...`. API keys can also go in `X-API-Key` (per the CreatedApiKeyResponse description). API keys with scope `read` may only use GET/HEAD/OPTIONS.
- Public (no security): health, register, login, 2fa/verify, password forgot/reset, email verify/confirm, oauth providers/login/callback/exchange, passkey login begin/finish, qr sessions create/poll, avatars serve. Everything else needs bearer.
- Tags: health, auth, oauth, passkeys, qr-login, users, avatars, media, api-keys, audit-log.

## 2. Paths and conventions
- Prefix `/api/v1` for everything except `/health/*`. Version in the URL.
- Plural kebab-case collections (`/users`, `/api-keys`, `/media`). `/audit-log` is the exception: singular. Path params are snake_case: `{id}`, `{provider}`, `{file}`.
- operationId is the bare handler name (`list`, `create`, `get_one`, `update`, `remove`, `revoke`, `me`). It is NOT unique across tags, so a generator should not rely on it (prefixing with the tag, e.g. `users_list`, would be better).
- No `summary` fields; human text lives only in response `description`s.
- Custom actions are POST sub-resources (`/qr/sessions/{id}/approve`, `/oauth/{provider}/link`). `me` alias: `/users/me`.

| Tag | Method Path | operationIds |
|---|---|---|
| users | GET/POST /api/v1/users; GET/PATCH/DELETE /api/v1/users/{id}; GET/PATCH /api/v1/users/me | list, create, get_one, update, remove, me, update_me |
| avatars | PUT/DELETE /api/v1/users/me/avatar; GET /api/v1/avatars/{file} | upload, remove, serve |
| media | GET/POST /api/v1/media; GET/DELETE /api/v1/media/{id}; GET /api/v1/media/{id}/content | list, upload, metadata, remove, content |
| api-keys | GET/POST /api/v1/api-keys; DELETE /api/v1/api-keys/{id} | list, create, revoke |
| audit-log | GET /api/v1/audit-log | list |
| auth | POST register, login, invitations, password/{forgot,reset,change}, email/{verify,change,change/confirm,verification/resend}, 2fa/{setup,enable,disable,verify}; GET me (all under /api/v1/auth) | |
| oauth | GET providers, identities, {provider}/login (307), {provider}/callback (307); POST exchange, {provider}/link; DELETE {provider} | |
| passkeys | GET /auth/passkeys; POST register/{begin,finish}, login/{begin,finish}; DELETE /auth/passkeys/{id} | |
| qr-login | POST /auth/qr/sessions; GET /auth/qr/sessions/{id}; POST .../{scan,approve,reject} | |
| health | GET /health/live, /health/ready | liveness, readiness |

## 3. CRUD pattern (users is the most complete; media and api-keys are partial)
- **List** `GET /users?page=&per_page=&q=&sort=&order=` returns 200 `PaginatedResponse_UserResponse`. Pagination is page-based (int32 >= 0, defaults not documented). `q` is free-text search. `sort` is an enum of columns (`UserSort`) and `order` is `SortOrder` (presumably asc/desc). Both are **referenced but not defined** in components, so the spec has dangling `$ref`s. There are no per-field filters. media and audit-log have only page/per_page and are sorted newest first. api-keys list is a bare unpaginated array.
- **Get** `GET /users/{id}` returns 200, 403 (not allowed) or 404.
- **Create** `POST /users` takes `CreateUserDto` and returns 201 + `UserResponse` (no Location header documented), or 409 on a duplicate email.
- **Update** is PATCH only (no PUT for resources): `UpdateUserDto`, every field optional, absent means unchanged. Returns 200 + the full resource, or 400 (business rule) / 404. PUT is used only for a binary replace (avatar).
- **Delete** `DELETE /users/{id}` returns 204 with an empty body, 400 on a business rule, 404 if missing. Users are **soft-deleted** ("User soft-deleted"). There is no restore endpoint and no include-deleted flag. api-keys delete is a "revoke".
- There are no bulk operations, no nested resource collections (ownership is implicit via the caller: media and api-keys are scoped to the current user, and someone else's id returns 404), and no ETag/If-Match.
- 401 is returned live but not documented per operation. 422 appears only for an undecodable avatar image. 413/415 for uploads. 202 for fire-and-forget email actions.

## 4. Response envelope
- **No envelope.** A single resource is returned as the bare object. A list is either `{items, page, per_page, total}` (`total` int64) or a bare array. No `success`, `data` or `meta` wrapper.
- Create returns 201 with the object. Delete returns 204 with no body. Actions return `MessageResponse {message}` (200/202).
- Generics are monomorphised as `PaginatedResponse_<Item>` with the item schema inlined.

## 5. Errors
- **No error schema in the spec**; error responses have only a description, no content. Live shape:
  `{"error":{"code":"<snake_case>","message":"<human text>"}}`
- Codes seen live: `unauthorized` ("missing Authorization header", "token is invalid or expired") and `bad_request` (JSON deserialize failures, wrong Content-Type). Codes probably follow the status (`not_found`, `conflict`, `forbidden`, ...), but this is unverified.
- There is no per-field validation details array. Deserialization errors are a single serde message string, and validation failures (weak password etc.) use 400, not 422.
- Statuses used: 200, 201, 202, 204, 307, 400, 401, 403, 404, 409, 413, 415, 422, 503. 412/428/429/500 are not documented.
- An unmatched route returns 404 with an **empty body** (no JSON).

## 6. Schema naming and types
- Requests: `<Verb><Noun>Dto` (`CreateUserDto`, `UpdateUserDto`, `UpdateProfileDto`, `CreateApiKeyDto`, `CredentialsDto`). Responses: `<Noun>Response` (`UserResponse`, `MediaResponse`, `ApiKeyResponse`). `CreatedApiKeyResponse` = allOf base + `key`.
- Fields are snake_case. ids are `string/uuid` (QR session id is a plain string). Timestamps are `string/date-time` (RFC 3339) and named `*_at`. Booleans use `is_*`/`has_*`/`*_enabled`/`*_verified`. Integers carry `format` int32/int64 and `minimum: 0` for unsigned.
- Nullable uses the 3.1 form `type: [T, "null"]`; an optional enum uses `oneOf [{type:null}, $ref]`. Nullable fields are left out of `required`.
- Enums are lowercase strings (`Role` user|admin, `ApiKeyScope` read|write, `Provider` google|github). QR `status` is an untyped string documented in prose.
- Secrets are never returned. Responses have derived fields (`has_password`, `avatar_url`, `key_prefix`, `url`) instead of raw columns.

## 7. Headers
- `Authorization: Bearer <jwt|ak_key>` or `X-API-Key: <key>`.
- Live: every app response carries `X-Request-Id` (uuid). It is not in the spec. `Vary: origin` indicates CORS.
- No ETag/If-Match, Idempotency-Key, rate-limit, Link or cursor headers. Login lockout exists (401 "temporarily locked") but no 429 is documented.
- `Content-Type: application/json` is required for JSON bodies (400 otherwise). Uploads are raw `application/octet-stream` (not multipart), with the filename in the query (`?filename=`).

## 8. Health and docs
- `GET /health/live` returns `{"status":"ok"}`. `GET /health/ready` returns `{"status":"ready"}`, or 503 if the database is unreachable. Neither is versioned. The body schema is not in the spec.
- Spec at `/api-docs/openapi.json`. Swagger UI at `/docs/` (`/docs` redirects with 303). `/api-docs` itself and `/swagger-ui/` return 404.

## 9. Not derivable from SQL alone
- The whole auth surface: register/login/JWT, 2FA TOTP + recovery codes, password reset, email verify/change tokens, invitations, OAuth redirect flows (307), WebAuthn passkeys, QR login state machine and polling.
- Authorization rules: admin vs user, owner scoping (media, api-keys, passkeys), API-key scope by HTTP method, 403 on viewing another user.
- Business rules: cannot delete self or the last admin, cannot remove the last sign-in method, lockout, weak/breached password check.
- Write-only and secret columns (password_hash, key hash, totp secret) that are hidden from responses, and computed response fields (`has_password`, `avatar_url`, `url`, `key_prefix`, parsed `details` JSON).
- Binary upload and download (avatar decode/resize to JPEG, media content sniffing, size limits 413/415, S3/local storage).
- Side effects: audit-log writes, emails, 202 semantics.
- Different request DTOs per role (`UpdateProfileDto` for self vs `UpdateUserDto` for admin), the `me` aliases, custom action routes (`revoke`, `approve`, `scan`, `reject`).
- Which columns are searchable by `q` and sortable by `sort`, and the default sort order (newest first).
- Soft delete (needs a `deleted_at` convention or annotation).

## 10. Resources a schema.sql needs
See derived-schema.sql. CRUD-shaped tables: users (full CRUD), media (list/get/create/delete), api_keys (list/create/delete), audit_log (read-only list). Supporting tables (no generic CRUD): oauth_identities, passkeys, totp_recovery_codes, one_time_tokens, qr_login_sessions.
To reproduce the endpoints, a generator needs per-table annotations at minimum: exposed ops, hidden/write-only columns, read-only/server-set columns, soft delete, searchable/sortable columns, owner column, admin-only, and pagination on/off.
