-- name: CreateCustomer :one
INSERT INTO customers (id, email, full_name)
VALUES ($1, $2, $3)
RETURNING *;

-- name: ListOrdersCursor :many
SELECT id, customer_id, status, ordered_at
FROM orders
WHERE customer_id = $1 AND ordered_at < $2
ORDER BY ordered_at DESC
LIMIT $3;

-- name: UpdateOrderStatus :exec
UPDATE orders SET status = $2 WHERE id = $1;

-- name: CreateOrderWithItems :exec
BEGIN;
INSERT INTO orders (id, customer_id, status)
VALUES ($1, $2, $3);
INSERT INTO order_items (order_id, product_id, quantity, unit_price_cents)
VALUES ($1, $4, $5, $6);
COMMIT;
