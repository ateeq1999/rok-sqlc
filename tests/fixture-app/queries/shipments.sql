-- Generated into tests/fixture-app/src/db/queries/shipments.rs: comparing a
-- value-list column to a placeholder makes its enum the parameter type.

-- name: ShipmentsWithStatus :many
SELECT id, status, carrier
FROM app.shipments
WHERE status = $1
ORDER BY id;
