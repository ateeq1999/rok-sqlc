-- Two-factor sign-in's SQL. `rok-sqlc g` generates
-- `src/db/queries/two_factor.rs`, one function per query, which only the
-- `two_factor` feature calls. `auth`'s sign-in issues the pending token
-- (`shared/security/auth_token.rs`); everything after it is here.

-- name: PendingSignIn :maybe_one
-- Whose sign-in the unused, unexpired `two_factor_pending` token whose
-- hash is `$1` waits on, without spending it: a wrong code leaves it
-- usable until it expires.
SELECT user_id
FROM auth_tokens
WHERE token_hash = $1 AND purpose = 'two_factor_pending' AND used_at IS NULL
    AND expires_at > now();

-- name: StoreTotpSecret :execrows
-- A two-factor setup for the live account `$1`, which hasn't enabled it:
-- its new encrypted secret, waiting for a first code. Nothing (`0`) when
-- two-factor is already on.
UPDATE users
SET totp_secret = $2, totp_last_step = NULL
WHERE id = $1 AND deleted_at IS NULL AND NOT totp_enabled;

-- name: EnableTotp :execrows
-- The live account `$1` confirmed its setup, whose encrypted secret is
-- `$3`, with the code of step `$2`: two-factor is on. Nothing (`0`) when
-- it already was, or another setup replaced that secret meanwhile.
-- param: $2 step i64
UPDATE users
SET totp_enabled = true, totp_last_step = $2
WHERE id = $1 AND deleted_at IS NULL AND NOT totp_enabled AND totp_secret = $3;

-- name: AcceptTotpStep :execrows
-- Spends the time step `$2` of the account `$1`'s codes: nothing (`0`)
-- when a code of that step or a later one was accepted already, so no code
-- is accepted twice, even by two requests at once.
-- param: $2 step i64
UPDATE users
SET totp_last_step = $2
WHERE id = $1 AND (totp_last_step IS NULL OR totp_last_step < $2);

-- name: DisableTotp :exec
-- Two-factor off for the account `$1`: its secret and last step forgotten,
-- and every session signed out, as a password change does.
UPDATE users
SET totp_enabled = false, totp_secret = NULL, totp_last_step = NULL,
    token_version = token_version + 1
WHERE id = $1;

-- name: DeleteRecoveryCodes :exec
-- Forgets every recovery code of the account `$1`.
DELETE FROM totp_recovery_codes
WHERE user_id = $1;

-- name: InsertRecoveryCode :exec
-- A new recovery code for the account `$2`: only its hash.
INSERT INTO totp_recovery_codes (id, user_id, code_hash)
VALUES ($1, $2, $3);

-- name: UseRecoveryCode :execrows
-- Spends the account `$1`'s unused recovery code whose hash is `$2`: `0`
-- when it has none such. One statement, so two requests can't both spend
-- it.
UPDATE totp_recovery_codes
SET used_at = now()
WHERE user_id = $1 AND code_hash = $2 AND used_at IS NULL;

-- name: RecordFailedCode :exec
-- A wrong code for the account `$1`, counted like a wrong password (see
-- `auth.sql`'s `RecordFailedLogin`): one more failure, or, at the `$2`-th,
-- a lock for `$3` minutes and a fresh count.
-- param: $2 max_failures i32
-- param: $3 lock_minutes i32
UPDATE users
SET failed_logins = CASE WHEN failed_logins + 1 >= $2 THEN 0 ELSE failed_logins + 1 END,
    locked_until = CASE
        WHEN failed_logins + 1 >= $2 THEN now() + make_interval(mins => $3)
        ELSE locked_until
    END
WHERE id = $1;

-- name: ClearFailedCodes :exec
-- A completed two-factor sign-in: the failures and any lock are forgotten.
UPDATE users
SET failed_logins = 0, locked_until = NULL
WHERE id = $1 AND (failed_logins <> 0 OR locked_until IS NOT NULL);
