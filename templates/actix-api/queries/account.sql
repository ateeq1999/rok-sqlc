-- The `account` feature's SQL: the signed-in user's own profile. `rok-sqlc
-- g` generates `src/db/queries/account.rs`; only the `account` feature
-- calls it.

-- name: UpdateDisplayName :maybe_one
-- Sets the live account `$1`'s display name (`NULL` clears it).
UPDATE users
SET display_name = $2
WHERE id = $1 AND deleted_at IS NULL
RETURNING *;

-- name: LockAvatarKey :maybe_one
-- The live account `$1`'s avatar key (`NULL` without a photo), its row
-- locked until the transaction ends, so two uploads can't both replace
-- the same photo.
SELECT avatar_key
FROM users
WHERE id = $1 AND deleted_at IS NULL
FOR UPDATE;

-- name: SetAvatarKey :maybe_one
-- Sets the live account `$1`'s avatar key (`NULL` clears it).
UPDATE users
SET avatar_key = $2
WHERE id = $1 AND deleted_at IS NULL
RETURNING *;
