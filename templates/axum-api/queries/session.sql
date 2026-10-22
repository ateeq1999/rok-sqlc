-- The session check's SQL. `rok-sqlc g` generates `src/db/queries/session.rs`;
-- `shared/auth.rs` calls it on every request. It is named after no feature,
-- so `shared/` doesn't reach into the `auth` feature's queries.

-- name: SessionState :maybe_one
-- What `AuthUser` checks on every request: the live account's token
-- version, whether it is active, and its role. A soft-deleted account has
-- none.
SELECT token_version, is_active, role
FROM users
WHERE id = $1 AND deleted_at IS NULL;

-- name: ApiKeySession :maybe_one
-- What `AuthUser` checks for a request sending an API key: the key whose
-- hash is `$1`, and its live owner's active flag and role. A soft-deleted
-- owner's key has none.
SELECT k.id, k.user_id, k.scope, k.expires_at, k.last_used_at, k.revoked_at, u.is_active, u.role
FROM api_keys k
JOIN users u ON u.id = k.user_id
WHERE k.key_hash = $1 AND u.deleted_at IS NULL;

-- name: TouchApiKey :exec
-- A key was used: its `last_used_at`, at most once a minute (the
-- database's clock), so authenticating doesn't write on every request.
UPDATE api_keys
SET last_used_at = now()
WHERE id = $1 AND (last_used_at IS NULL OR last_used_at < now() - interval '1 minute');
