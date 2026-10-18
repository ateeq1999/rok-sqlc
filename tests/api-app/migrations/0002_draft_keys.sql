-- A schema change that adds endpoints to an existing feature: with a key,
-- `drafts` gains `GET`, `PATCH` and `DELETE /{id}`, and its service trait
-- the methods behind them, while the user's `service.rs` stays as it is.
ALTER TABLE drafts ADD COLUMN id UUID PRIMARY KEY;
