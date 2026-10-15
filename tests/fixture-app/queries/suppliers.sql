-- A second query file, generated into tests/fixture-app/src/db/queries/suppliers.rs.
-- `SearchProducts` deliberately shares its name with a query in inventory.sql:
-- function names only need to be unique within one query file.

-- name: CreateSupplier :one
INSERT INTO suppliers (id, name)
VALUES ($1, $2)
RETURNING *;

-- name: SearchProducts :many
SELECT p.sku
FROM products p
JOIN suppliers s ON s.id = p.supplier_id
WHERE s.name = $1
ORDER BY p.sku;
