-- name: CreateProduct :one
INSERT INTO products (id, sku, name, category_id, supplier_id, price_cents, stock_on_hand, reorder_level, weight_kg, discontinued, released_on)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
RETURNING *;

-- name: CreateProducts :execrows
INSERT INTO products (id, sku, name, category_id, price_cents)
VALUES ($1, $2, $3, $4, $5), ($6, $7, $8, $9, $10);

-- name: UpdateProductStock :exec
UPDATE products SET stock_on_hand = $2 WHERE id = $1;

-- name: DeleteProduct :exec
DELETE FROM products WHERE id = $1;

-- name: ListProductsByCategory :many
SELECT id, sku, name, category_id, supplier_id, price_cents, stock_on_hand, reorder_level, weight_kg, discontinued, released_on, created_at
FROM products
WHERE category_id = $1 AND stock_on_hand <= $2
ORDER BY sku;

-- name: ListProductsOffset :many
SELECT id, sku, name, price_cents
FROM products
ORDER BY id
LIMIT $1 OFFSET $2;

-- name: UpsertProduct :exec
INSERT INTO products (id, sku, name, category_id, price_cents)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (sku) DO UPDATE SET name = EXCLUDED.name, price_cents = EXCLUDED.price_cents;
