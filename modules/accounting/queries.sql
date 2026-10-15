-- name: CreateAccount :one
INSERT INTO accounts (id, code, name)
VALUES ($1, $2, $3)
RETURNING *;

-- name: ListAccounts :many
SELECT id, code, name, is_active
FROM accounts
WHERE is_active = $1
ORDER BY code;

-- name: DeactivateAccount :exec
UPDATE accounts SET is_active = FALSE WHERE id = $1;
