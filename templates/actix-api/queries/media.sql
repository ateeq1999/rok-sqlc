-- The `media` feature's SQL: each caller's uploaded files. `rok-sqlc g`
-- generates `src/db/queries/media.rs`; only the `media` feature calls it.
-- Every lookup is scoped to the owner, so another caller's file is never
-- found.

-- name: InsertMedia :one
-- A stored file's row; its bytes are already under `storage_key`.
INSERT INTO media (id, owner_id, content_type, size_bytes, original_filename, storage_key)
VALUES ($1, $2, $3, $4, $5, $6)
RETURNING *;

-- name: OwnedMedia :maybe_one
-- The file `$1`, when `$2` owns it.
SELECT *
FROM media
WHERE id = $1 AND owner_id = $2;

-- name: ListOwnedMedia :many
-- One page of `$1`'s files, newest first.
-- param: $2 limit i64
-- param: $3 offset i64
SELECT *
FROM media
WHERE owner_id = $1
ORDER BY created_at DESC, id DESC
LIMIT $2 OFFSET $3;

-- name: CountOwnedMedia :one
-- How many files `$1` has.
SELECT count(*) AS total
FROM media
WHERE owner_id = $1;

-- name: DeleteOwnedMedia :maybe_one
-- Deletes the file `$1`'s row, when `$2` owns it: the key of its bytes.
DELETE FROM media
WHERE id = $1 AND owner_id = $2
RETURNING storage_key;
