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

**Status:** ready-for-agent

- [ ] PostgreSQL tests, in both templates:
  - create → use as Bearer and as `X-API-Key` → list shows `last_used_at`;
  - a read key's POST → 403;
  - a key on `/auth/me` and `/api-keys` → 403;
  - an expired key → 401; a revoked key → 401;
  - an inactive owner → 401;
  - another user's key delete → 404;
  - the 26th key → the chosen error;
  - only the hash is stored.
- [ ] Gates pass, including both templates' checks and the drift test.
