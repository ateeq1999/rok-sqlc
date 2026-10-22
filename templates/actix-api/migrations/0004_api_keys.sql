-- API keys (the `api_keys` feature). Every request sending a key looks it
-- up by its hash, and a user's active keys are listed and counted.

CREATE UNIQUE INDEX api_keys_key_hash_key ON api_keys (key_hash);

CREATE INDEX api_keys_user_id_active_idx ON api_keys (user_id) WHERE revoked_at IS NULL;
