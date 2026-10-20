-- The account flows' SQL. `rok-sqlc g` generates `src/db/queries/auth.rs`,
-- one function per query, which only the `auth` feature calls (the session
-- check `shared/auth.rs` runs is in `session.sql`).

-- name: FindLogin :maybe_one
-- The live account signing in with `email`, whatever its case.
-- param: $1 email String
SELECT id, password_hash, role, is_active, email_verified, token_version, failed_logins, locked_until
FROM users
WHERE lower(email) = lower($1) AND deleted_at IS NULL;

-- name: CreateUser :one
-- A new account signing up with `email` and its password hash; the role
-- and the flags take their defaults (`user`, active, unverified). A live
-- account with the same email, whatever its case, is a unique violation
-- of `users_email_key`.
INSERT INTO users (id, email, password_hash)
VALUES ($1, $2, $3)
RETURNING *;

-- name: RecordFailedLogin :exec
-- A wrong password for the account `$1`: one more failure, or, at the
-- `$2`-th, a lock for `$3` minutes (the database's clock) and a fresh count.
-- param: $2 max_failures i32
-- param: $3 lock_minutes i32
UPDATE users
SET failed_logins = CASE WHEN failed_logins + 1 >= $2 THEN 0 ELSE failed_logins + 1 END,
    locked_until = CASE
        WHEN failed_logins + 1 >= $2 THEN now() + make_interval(mins => $3)
        ELSE locked_until
    END
WHERE id = $1;

-- name: ClearFailedLogins :exec
-- A successful sign-in: the failures and any lock are forgotten.
UPDATE users
SET failed_logins = 0, locked_until = NULL
WHERE id = $1 AND (failed_logins <> 0 OR locked_until IS NOT NULL);

-- name: MarkEmailVerified :execrows
-- The live account `$1` proved it owns its email.
UPDATE users
SET email_verified = true
WHERE id = $1 AND deleted_at IS NULL;

-- name: RevokeTokens :exec
-- Spends every unused token of the account `$1` for `$2`: a new one
-- replaces them.
UPDATE auth_tokens
SET used_at = now()
WHERE user_id = $1 AND purpose = $2 AND used_at IS NULL;

-- name: InsertToken :exec
-- A new single-use token: only its hash, valid until `expires_at`.
INSERT INTO auth_tokens (id, user_id, purpose, token_hash, new_email, expires_at)
VALUES ($1, $2, $3, $4, $5, $6);

-- name: ConsumeToken :maybe_one
-- Spends the unused, unexpired token whose hash is `$1`, if it was issued
-- for `$2`, and answers whom it was for. One statement, so two requests
-- can't both spend it.
UPDATE auth_tokens
SET used_at = now()
WHERE token_hash = $1 AND purpose = $2 AND used_at IS NULL AND expires_at > now()
RETURNING user_id, new_email;

-- name: LiveTokenPurpose :maybe_one
-- What the unused, unexpired token whose hash is `$1` was issued for,
-- without spending it: a flow checks the link before its slow work (the
-- breach check, hashing), then spends it with `ConsumeToken`.
SELECT purpose
FROM auth_tokens
WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now();

-- name: ResetPassword :execrows
-- A reset (or invitation) link redeemed for the live account `$1`: its
-- new password hash, every older session signed out, the failures and any
-- lock forgotten. `$3` (an invitation) also marks the email verified, since
-- the link reached it.
-- param: $3 verify_email bool
UPDATE users
SET password_hash = $2,
    token_version = token_version + 1,
    failed_logins = 0,
    locked_until = NULL,
    email_verified = email_verified OR $3
WHERE id = $1 AND deleted_at IS NULL;

-- name: ChangePassword :execrows
-- The live account `$1` chose a new password: its hash, and every session
-- (the current one included) signed out.
UPDATE users
SET password_hash = $2,
    token_version = token_version + 1
WHERE id = $1 AND deleted_at IS NULL;

-- name: ChangeEmail :execrows
-- The live account `$1` confirmed `$2` as its new email, which it now owns
-- (verified). A live account already using it, whatever its case, is a
-- unique violation of `users_email_key`.
UPDATE users
SET email = $2, email_verified = true
WHERE id = $1 AND deleted_at IS NULL;

-- name: InviteUser :one
-- An account an admin invites: `email`, a display name and a role, but no
-- password and an unverified email until the invitee follows the link. A
-- live account with the same email, whatever its case, is a unique
-- violation of `users_email_key`.
INSERT INTO users (id, email, display_name, role)
VALUES ($1, $2, $3, $4)
RETURNING *;
