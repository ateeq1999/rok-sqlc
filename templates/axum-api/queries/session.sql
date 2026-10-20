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
