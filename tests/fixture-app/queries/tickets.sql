-- Generated into tests/fixture-app/src/db/queries/tickets.rs: comparing an
-- enum column to a placeholder, or casting the placeholder to an enum type,
-- makes the enum the parameter type.

-- name: TicketsWithPriority :many
SELECT id, priority, state
FROM app.tickets
WHERE priority = $1
ORDER BY id;

-- name: TicketsInStates :many
SELECT id, state, $2::ticket_priority AS wanted
FROM app.tickets
WHERE state = ANY($1::app.ticket_state[])
ORDER BY id;

-- name: ContactsOnChannel :many
SELECT id
FROM ticket_contacts
WHERE channel = $1::contact_channel OR $1::contact_channel = ANY(fallbacks)
ORDER BY id;
