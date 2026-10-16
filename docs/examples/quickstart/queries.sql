-- name: ActiveProductsInCategory :many
SELECT id, sku, name, price
FROM products
WHERE category_id = $1 AND status = 'active'
ORDER BY name, id;

-- name: ProductCountsByCategory :many
SELECT c.name, COUNT(p.id) AS product_count
FROM categories c
LEFT JOIN products p ON p.category_id = c.id
GROUP BY c.name
ORDER BY c.name;

-- name: UpsertProductPrice :one
INSERT INTO products (category_id, sku, name, price)
VALUES ($1, $2, $3, $4)
ON CONFLICT (sku) DO UPDATE SET price = EXCLUDED.price
RETURNING *;

-- name: SearchProductNames :many
-- param: $1 pattern String
-- column: display_name String
SELECT sku, upper(name) AS display_name
FROM products
WHERE lower(name) LIKE lower($1)
ORDER BY sku;

-- name: ArchiveCategory :execrows
UPDATE products SET status = 'archived' WHERE category_id = $1;
