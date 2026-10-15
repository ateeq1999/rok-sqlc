# Typed search: filters, allowlisted sorting, and keyset pagination

Every resource gets a typed search API, generated without configuration:

- `filter.rs` holds `XFilter`, with one condition list per filterable column (all conditions are combined with `AND`), plus an `x_is_null: Option<bool>` check for every nullable column. It also holds `XSortKey`, an enum of the columns that may be sorted by.
- `read::search(executor, filter, sort, page)` returns one page in `sort` order, breaking ties by primary key so that pages are stable.
- `read::search_after(executor, filter, after, limit)` does keyset pagination in primary-key order: `after` is the last key seen (a tuple for composite keys, compared as a row). It is generated only for tables with a primary key.
- `db/search.rs` holds the shared types once: `Page` (limit clamped to `1..=MAX_PAGE_SIZE`, 500), `Sort<K>`/`Direction`, and the condition enums.

Conditions are typed by column: `Ordered<T>` for numbers, decimals, dates, and times (`Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge`, `Between`, `In`, `NotIn`); `Text` for strings (`Eq`, `Ne`, `In`, `NotIn`, `Contains`, `StartsWith`, `EndsWith`); and `Equality<T>` for booleans and UUIDs. Callers can only name columns through the generated fields and enum variants, and every value is bound, so no caller string reaches the SQL text. The substring variants escape `%`, `_`, and `\` so they match literally. Array and JSON columns get only null checks: JSONB operators should exist only when explicitly configured, and no configuration exists yet.

The feature is always on. A `rok-sqlc.toml` is deferred until something is genuinely opt-in (full-text search, pgvector); an unused filter costs nothing at runtime.

`schema` and `search` become reserved module names: a table that maps to either is reported as a failure instead of generating colliding modules.

## Considered Options

- **Configure filterable/sortable columns in `rok-sqlc.toml`.** Rejected for now: it adds new CLI surface and a parser to maintain for little benefit, since every column is safe to filter and sort by.
- **One `Option<Condition>` per column.** Rejected: ranges need two conditions (`Ge` and `Lt`), and a `Vec` expresses that without a special case.
- **Keyset pagination over arbitrary sort keys.** Deferred: the cursor would need the sort value as well as the key, typed per sort key. Primary-key keysets cover the common "walk everything" case; `search` with `Page` covers sorted browsing.
- **Rejecting out-of-range page sizes with an error.** Rejected in favor of clamping in `Page::new`. The `Page` type makes the bound visible, and an error type would leak into every signature.
