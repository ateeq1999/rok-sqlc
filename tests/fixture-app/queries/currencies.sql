-- Generated into tests/fixture-app/src/db/queries/currencies.rs: placeholders
-- compared with a key column take its typed id, borrowed for a text key
-- (`&CurrencyId`), and result columns drawn from one decode as the id.

-- name: CurrencyName :maybe_one
SELECT name FROM currencies WHERE code = $1;

-- name: RatesForCurrencies :many
SELECT currency_code, day, rate
FROM currency_rates
WHERE currency_code = ANY($1)
ORDER BY day;
