-- The `account` feature's SQL: the signed-in user's own profile. `rok-sqlc
-- g` generates `src/db/queries/account.rs`; only the `account` feature
-- calls it.

-- name: UpdateDisplayName :maybe_one
-- Sets the live account `$1`'s display name (`NULL` clears it).
UPDATE users
SET display_name = $2
WHERE id = $1 AND deleted_at IS NULL
RETURNING *;
