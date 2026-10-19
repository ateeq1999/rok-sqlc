-- The account flows' SQL. `rok-sqlc g` generates `src/db/queries/auth.rs`,
-- one function per query; the `auth` feature and `shared/` call them.

-- name: SessionState :maybe_one
-- What `AuthUser` checks on every request: the live account's token
-- version, whether it is active, and its role. A soft-deleted account has
-- none.
SELECT token_version, is_active, role
FROM users
WHERE id = $1 AND deleted_at IS NULL;

-- name: FindLogin :maybe_one
-- The live account signing in with `email`, whatever its case.
-- param: $1 email String
SELECT id, password_hash, role, is_active, email_verified, token_version, failed_logins, locked_until
FROM users
WHERE lower(email) = lower($1) AND deleted_at IS NULL;
