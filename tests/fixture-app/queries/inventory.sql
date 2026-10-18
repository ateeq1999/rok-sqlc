-- Annotated queries generated into tests/fixture-app/src/db/queries/inventory.rs
-- by tests/fixture_compile/; tests/fixture-app/tests/postgres.rs runs them.

-- name: CreateCategory :one
INSERT INTO categories (name, description)
VALUES ($1, $2)
RETURNING *;

-- name: GetProductBySku :maybe_one
SELECT id, sku, name, price_cents
FROM products
WHERE sku = $1;

-- name: ListProductsByCategory :many
SELECT id, sku, name, price_cents
FROM products
WHERE category_id = $1 AND stock_on_hand <= $2
ORDER BY name, id;

-- name: ListProductsWithCategory :many
SELECT p.sku, c.name AS category_name, s.name AS supplier_name
FROM products p
JOIN categories c ON c.id = p.category_id
LEFT JOIN suppliers s ON s.id = p.supplier_id
WHERE p.sku = ANY($1)
ORDER BY p.sku;

-- name: CountProductsInCategory :one
SELECT COUNT(*) FROM products WHERE category_id = $1;

-- name: CreateProducts :execrows
INSERT INTO products (id, sku, name, category_id, supplier_id)
VALUES ($1, $2, $3, $4, $5), ($6, $7, $8, $9, $10);

-- name: UpdateProductStock :exec
UPDATE products SET stock_on_hand = $2 WHERE id = $1;

-- name: CategoryStock :many
-- column: total_stock i64
SELECT category_id, SUM(stock_on_hand)::bigint AS total_stock
FROM products
GROUP BY category_id
ORDER BY category_id;

-- name: SearchProducts :many
-- param: $1 pattern String
SELECT sku
FROM products
WHERE lower(name) LIKE lower($1)
ORDER BY sku;

-- name: DeleteProductBySku :execrows
DELETE FROM products WHERE sku = $1;

-- name: StockedCategories :many
WITH stocked (category_id, product_count, total_stock) AS (
    SELECT category_id, COUNT(*), SUM(stock_on_hand)::bigint
    FROM products
    WHERE stock_on_hand >= $1
    GROUP BY category_id
)
SELECT c.id, c.name AS category_name, s.product_count, s.total_stock
FROM categories c
JOIN stocked s ON s.category_id = c.id
WHERE c.id = $2
ORDER BY c.id;
