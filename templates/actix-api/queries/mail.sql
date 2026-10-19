-- The mail outbox's SQL. `rok-sqlc g` generates `src/db/queries/mail.rs`;
-- `shared/mail/` calls it: flows enqueue a mail on their transaction, and
-- the sender claims, sends and settles it.

-- name: EnqueueMail :exec
-- A rendered mail, due now.
INSERT INTO outbound_mail (id, payload)
VALUES ($1, $2);

-- name: ClaimDueMail :many
-- Up to `$1` pending, due rows, oldest first, locked until the claiming
-- transaction ends. Rows another sender holds are skipped, so two senders
-- never send the same row.
-- param: $1 batch i64
SELECT id, payload, attempts
FROM outbound_mail
WHERE status = 'pending' AND next_attempt_at <= now()
ORDER BY next_attempt_at, id
LIMIT $1
FOR UPDATE SKIP LOCKED;

-- name: DeleteSentMail :exec
-- A sent mail: nothing is left to do.
DELETE FROM outbound_mail
WHERE id = $1;

-- name: RecordFailedMail :exec
-- A failed attempt: its count and error, and a retry `$5` seconds from now
-- (the database's clock), or `dead` when `$4` says so.
-- param: $5 retry_in_secs f64
UPDATE outbound_mail
SET attempts = $2, last_error = $3, status = $4,
    next_attempt_at = now() + make_interval(secs => $5)
WHERE id = $1;
