-- Generated into tests/fixture-app/src/plain_db/queries.rs: casts to
-- `interval`, `bytea` and `timetz` infer `PgInterval`, `Vec<u8>` (bound as
-- `&[u8]`) and `PgTimeTz`.

-- name: BadgesAtLeast :many
SELECT id, badge, (span + $1::interval)::interval AS extended
FROM timed_badges
WHERE span >= $2::interval AND badge <> $3::bytea
ORDER BY id;

-- name: BadgeOpening :maybe_one
SELECT opens FROM timed_badges WHERE opens = $1::timetz;
