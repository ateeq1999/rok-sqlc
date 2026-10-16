-- Generated into tests/fixture-app/src/db/queries/tickets.rs: comparing an
-- enum column to a placeholder makes the enum the parameter type.

-- name: TicketsWithPriority :many
SELECT id, priority, state
FROM app.tickets
WHERE priority = $1
ORDER BY id;
