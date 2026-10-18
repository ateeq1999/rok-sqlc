-- Derived from api-starter-axum OpenAPI 0.1.0 (PostgreSQL).
-- Exposed columns come from the response/request DTOs; hidden columns (hashes, secrets,
-- storage keys) are inferred from documented behaviour, not from the spec.

CREATE TYPE user_role AS ENUM ('user', 'admin');
CREATE TYPE api_key_scope AS ENUM ('read', 'write');
CREATE TYPE oauth_provider AS ENUM ('google', 'github');

-- /api/v1/users: list(page, per_page, q, sort, order), get, create, patch, delete (soft)
CREATE TABLE users (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email           text NOT NULL,                  -- unique -> 409
    display_name    text,
    password_hash   text,                           -- write-only; has_password = password_hash IS NOT NULL
    role            user_role NOT NULL DEFAULT 'user',
    is_active       boolean NOT NULL DEFAULT true,
    email_verified  boolean NOT NULL DEFAULT false,
    totp_secret     text,                           -- hidden
    totp_enabled    boolean NOT NULL DEFAULT false,
    avatar_key      text,                           -- exposed as avatar_url
    failed_logins   integer NOT NULL DEFAULT 0,     -- lockout (hidden)
    locked_until    timestamptz,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz                     -- soft delete
);
CREATE UNIQUE INDEX users_email_key ON users (lower(email)) WHERE deleted_at IS NULL;

-- /api/v1/media: list, get, create (binary upload), delete, + /{id}/content; owner-scoped
CREATE TABLE media (
    id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id          uuid NOT NULL REFERENCES users(id),
    content_type      text NOT NULL,
    size_bytes        bigint NOT NULL,
    original_filename text,
    storage_key       text NOT NULL,                -- exposed as url
    created_at        timestamptz NOT NULL DEFAULT now()
);

-- /api/v1/api-keys: list (unpaginated), create, delete (revoke); owner-scoped
CREATE TABLE api_keys (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      uuid NOT NULL REFERENCES users(id),
    name         text NOT NULL,
    key_prefix   text NOT NULL,
    key_hash     text NOT NULL,                     -- hidden; raw key returned once on create
    scope        api_key_scope NOT NULL DEFAULT 'read',
    expires_at   timestamptz,
    last_used_at timestamptz,
    revoked_at   timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now()
);

-- /api/v1/audit-log: read-only paginated list, newest first, admin only
CREATE TABLE audit_log (
    id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_user_id  uuid NOT NULL REFERENCES users(id),
    target_user_id uuid REFERENCES users(id),
    action         text NOT NULL,
    details        jsonb,
    created_at     timestamptz NOT NULL DEFAULT now()
);

-- Supporting tables (custom endpoints, not generic CRUD)
CREATE TABLE oauth_identities (
    user_id    uuid NOT NULL REFERENCES users(id),
    provider   oauth_provider NOT NULL,
    subject    text NOT NULL,
    email      text,
    linked_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, provider),
    UNIQUE (provider, subject)
);

CREATE TABLE passkeys (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       uuid NOT NULL REFERENCES users(id),
    name          text NOT NULL,
    credential    jsonb NOT NULL,
    last_used_at  timestamptz,
    created_at    timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE totp_recovery_codes (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    uuid NOT NULL REFERENCES users(id),
    code_hash  text NOT NULL,
    used_at    timestamptz
);

-- email verification, email change, password reset, invitations
CREATE TABLE one_time_tokens (
    token_hash text PRIMARY KEY,
    user_id    uuid NOT NULL REFERENCES users(id),
    purpose    text NOT NULL,
    payload    text,                                -- e.g. new_email
    expires_at timestamptz NOT NULL,
    used_at    timestamptz
);

CREATE TABLE qr_login_sessions (
    id                 text PRIMARY KEY,
    poll_secret_hash   text NOT NULL,
    verification_code  text NOT NULL,
    status             text NOT NULL DEFAULT 'pending', -- pending|scanned|approved|rejected|expired|consumed
    approved_user_id   uuid REFERENCES users(id),
    requester_ip       text,
    requester_agent    text,
    requested_at       timestamptz NOT NULL DEFAULT now(),
    expires_at         timestamptz NOT NULL
);
