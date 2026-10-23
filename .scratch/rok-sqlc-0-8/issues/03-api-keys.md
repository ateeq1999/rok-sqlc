# 03: API keys (both templates)

**What to build:** decision B4, in both templates.

- **Feature `api_keys`** (hand-written, standard layout, user-owned; remove it from `[api] exclude` only if the generated CRUD isn't used; it should stay excluded, since these are hand-written):
  - `GET /api-keys`: the caller's active keys (not revoked, not expired), newest first.
  - `POST /api-keys { name, scope, expires_in_days? }`: answers `201 CreatedApiKeyResponse` with the raw key once. Over 25 active keys answers `409 conflict` (or a clearer `422`; pick and document).
  - `DELETE /api-keys/{id}`: sets `revoked_at` and answers `204`. Another user's or an unknown key answers `404`.
- **Key format and storage:** `rok_<8 chars>_<32 chars>` (base62), in `shared/security/api_key.rs`. Stores `key_prefix` and the SHA-256 `key_hash`. Queries in `queries/api_keys.sql`. An index on `key_prefix` (or `key_hash`) in a migration, if needed (coordinate numbering with 02's `0003`: use `0004_api_keys.sql`).
- **Authentication:** `AuthUser` (both frameworks' `shared/auth.rs`) accepts `Authorization: Bearer rok_…` or `X-API-Key`.
  - It looks up by hash, checks not revoked, not expired, and the owner live and active, and touches `last_used_at` at most once a minute.
  - A `read` key on a non-safe method answers `403`.
  - Any key on `/api/v1/auth/*` or `/api/v1/api-keys*` answers `403`.
  - Put the framework-free logic (parse, verify, rules) in `shared/security/` so it's shared byte-identically. `AuthUser` should say whether it came from a session or a key, if the handlers need that.
- `dto.rs`/`service.rs` are byte-identical across the templates (drift test). The contract covers the three paths.

**Blocked by:** None. It runs alongside 01 and 04. Ticket 02 also edits `features/auth` and `shared/security`; keep changes additive.

**Status:** resolved

- [x] PostgreSQL tests, in both templates:
  - create → use as Bearer and as `X-API-Key` → list shows `last_used_at`;
  - a read key's POST → 403;
  - a key on `/auth/me` and `/api-keys` → 403;
  - an expired key → 401; a revoked key → 401;
  - an inactive owner → 401;
  - another user's key delete → 404;
  - the 26th key → the chosen error;
  - only the hash is stored.
- [x] Gates pass, including both templates' checks and the drift test.

## Comments

Done on `rok-sqlc-0-8-03`.

- **Feature layout.** `src/features/api_keys/{mod,routes,handlers,dto,service}.rs` in both templates, declared in `features/mod.rs`; `dto.rs`/`service.rs` (and `mod.rs`) are byte-identical and `api_keys` is in the drift test's `HAND_WRITTEN`. `api_keys` stays in `[api] exclude`. Wiring is additive: axum `AppState.api_keys: Arc<ApiKeyService>` + `.merge(api_keys::routes())`; actix `configure` registers `web::Data<ApiKeyService>` and calls `api_keys::routes(cfg)`. `ApiDoc` lists `list`/`create`/`revoke` (tag `api-keys`); the contract's `COVERED` has the three operations, no `KNOWN` lines needed.
- **Shared security (`shared/security/api_key.rs`, byte-identical):** `NewApiKey::generate() -> { raw: rok_<8>_<32> base62, prefix: rok_<8> (12 chars, stored as key_prefix), hash: token::hash(raw) }`, `presented(authorization, x_api_key) -> Presented::{Session(jwt), ApiKey(key)}` (`Authorization` wins; a bearer starting `rok_` is a key; neither is `401 a bearer token is required`), `check_reach(scope, method, path)` (`SESSION_ONLY = ["/api/v1/auth", "/api/v1/api-keys"]` on segment boundaries, `read` = GET/HEAD/OPTIONS; both `403`), `authenticate(pool, raw, method, path) -> KeyOwner { id, role, scope }` (lookup by hash, revoked/expired/inactive or deleted owner `401`, then reach `403`, then touch), `Credential::{Session, ApiKey(scope)}`, `MAX_ACTIVE = 25`, `HEADER = "x-api-key"`.
- **AuthUser** gained `credential: Credential` (re-exported from `shared/auth.rs`), still `Copy`. Each framework's `shared/auth.rs` asks `presented` and either decodes the JWT as before or calls `api_key::authenticate` with the method and the whole path (axum reads `OriginalUri`, so a nested router still sees the full path; actix `req.path()`). `AdminUser` follows (an admin's write key passes).
- **The 26th key: `409 conflict`** ("you can have at most 25 active API keys; revoke one first"). Creating locks the owner's `users` row (`LockKeyOwner … FOR UPDATE`) before counting, so concurrent creates can't pass the limit. Validation is `422`: name trimmed, 1–100 chars; `expires_in_days` 1–365 (`Option<u32>`, so the schema keeps the target's `minimum: 0`); `scope` defaults to `read`.
- **Queries.** `queries/api_keys.sql` (feature-only): `ActiveApiKeys` (not revoked, not expired, newest first), `LockKeyOwner`, `CountActiveApiKeys`, `InsertApiKey` (`InsertApiKeyParams`, `RETURNING *`), `RevokeApiKey :execrows` (owner-scoped, unrevoked; `0` → `404`). `queries/session.sql` (what `shared/` runs per request): `ApiKeySession` (key joined with its live owner's `is_active`, `role`) and `TouchApiKey` (`last_used_at = now()` only when null or over a minute old). Migration `0004_api_keys.sql`: unique `api_keys_key_hash_key` and a partial `(user_id) WHERE revoked_at IS NULL` index; the unique index adds a generated `db::api_keys::read::get_by_key_hash` (unused).
- **Tests.** `tests/api_keys.rs` (8, both templates): Bearer + `X-API-Key` + list `last_used_at`; read key PATCH 403 / write key 200; `/auth/me`, `/api-keys` GET/POST/DELETE with a key 403; expired, revoked, inactive owner and unknown key 401 (revoked again 404); another user's delete 404; 26th 409 then revoke and create; 422 bounds; only prefix + hash stored (`row_to_json` holds no raw key). Unit tests in `api_key.rs` (format, credential parsing, reach rules). Test helper `support::call_headers(pool, &[(name, value)], method, uri, body)` (no bearer; any headers).
- **Left for 06:** the `rok-sqlc.toml` `[api] exclude` comment, `0001_init.sql`'s header and the README's "`media` and `api_keys` are in the schema for the features to come" line still say "later release" (left alone to avoid conflicts with 04); the README Accounts section and `AGENTS.md` describe API keys.
