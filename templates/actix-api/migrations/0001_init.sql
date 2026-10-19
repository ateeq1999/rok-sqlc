-- The schema of the target API (api-starter-axum's OpenAPI document), as
-- far as generation covers it. `users` and `audit_log` have API features;
-- `media` and `api_keys` are here because the hand-written features of a
-- later release use them, and `rok-sqlc.toml` excludes them from the API
-- for now.

CREATE TYPE role AS ENUM ('user', 'admin');
CREATE TYPE api_key_scope AS ENUM ('read', 'write');

-- Sets `updated_at` on every update.
CREATE FUNCTION touch_updated_at() RETURNS trigger
LANGUAGE plpgsql AS $$
begin
  new.updated_at := now();
  return new;
end;
$$;

CREATE TABLE users (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL,
    display_name TEXT,
    -- Write-only: set by the password flow, never answered.
    password_hash TEXT,
    has_password BOOLEAN NOT NULL GENERATED ALWAYS AS (password_hash IS NOT NULL) STORED,
    role role NOT NULL DEFAULT 'user',
    is_active BOOLEAN NOT NULL DEFAULT true,
    email_verified BOOLEAN NOT NULL DEFAULT false,
    totp_secret TEXT,
    totp_enabled BOOLEAN NOT NULL DEFAULT false,
    -- Answered as `avatar_url` once uploads exist.
    avatar_key TEXT,
    failed_logins INTEGER NOT NULL DEFAULT 0,
    locked_until TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);

-- One live account per email, whatever its case.
CREATE UNIQUE INDEX users_email_key ON users (lower(email)) WHERE deleted_at IS NULL;

CREATE TRIGGER users_touch_updated_at BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION touch_updated_at();

-- Administrative actions, newest first; written by the application.
CREATE TABLE audit_log (
    id UUID PRIMARY KEY,
    actor_user_id UUID NOT NULL REFERENCES users (id),
    target_user_id UUID REFERENCES users (id),
    action TEXT NOT NULL,
    details JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Uploaded files, each owned by its uploader.
CREATE TABLE media (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id),
    content_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    original_filename TEXT,
    storage_key TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- API keys: only a hash of the key is kept.
CREATE TABLE api_keys (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id),
    name TEXT NOT NULL,
    key_prefix TEXT NOT NULL,
    key_hash TEXT NOT NULL,
    scope api_key_scope NOT NULL DEFAULT 'read',
    expires_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
