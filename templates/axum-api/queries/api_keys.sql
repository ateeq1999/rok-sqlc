-- The `api_keys` feature's SQL: a user's own API keys. `rok-sqlc g`
-- generates `src/db/queries/api_keys.rs`; only the `api_keys` feature calls
-- it (the per-request check `shared/auth.rs` runs is in `session.sql`).

-- name: ActiveApiKeys :many
-- The user `$1`'s active keys (not revoked, not expired), newest first.
SELECT *
FROM api_keys
WHERE user_id = $1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > now())
ORDER BY created_at DESC, id DESC;

-- name: LockKeyOwner :maybe_one
-- Locks the live account `$1` until the transaction ends, so two requests
-- making keys for it count its active keys one after the other.
SELECT id
FROM users
WHERE id = $1 AND deleted_at IS NULL
FOR UPDATE;

-- name: CountActiveApiKeys :one
-- How many active keys the user `$1` has.
SELECT COUNT(*)
FROM api_keys
WHERE user_id = $1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > now());

-- name: InsertApiKey :one
-- A new key: its prefix and hash, never the key itself.
INSERT INTO api_keys (id, user_id, name, key_prefix, key_hash, scope, expires_at)
VALUES ($1, $2, $3, $4, $5, $6, $7)
RETURNING *;

-- name: RevokeApiKey :execrows
-- Revokes the user `$2`'s key `$1`, unless it is already revoked.
UPDATE api_keys
SET revoked_at = now()
WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL;
