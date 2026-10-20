-- What the account flows need: a token version on each user, single-use
-- tokens, and the mail outbox. `rok-sqlc.toml` keeps the two tables out of
-- the API; the auth features reach them through `queries/`.

-- Bumped by a password change, a reset or an admin disable. A token whose
-- `tv` claim no longer matches is rejected on its next request.
ALTER TABLE users ADD COLUMN token_version INTEGER NOT NULL DEFAULT 1;

-- Single-use tokens mailed as links. Only the SHA-256 of the token is
-- kept; `used_at` spends it, and issuing a new one for the same user and
-- purpose invalidates the older ones.
CREATE TABLE auth_tokens (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    purpose TEXT NOT NULL CHECK (
        purpose IN ('email_verification', 'password_reset', 'email_change', 'invitation')
    ),
    token_hash TEXT NOT NULL UNIQUE,
    -- The address an `email_change` token moves the account to.
    new_email TEXT,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX auth_tokens_user_purpose ON auth_tokens (user_id, purpose);

-- The mail outbox: a row is written on the flow's transaction and deleted
-- once sent. A row that keeps failing becomes `dead`, keeping its error.
CREATE TABLE outbound_mail (
    id UUID PRIMARY KEY,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'dead')),
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX outbound_mail_due ON outbound_mail (next_attempt_at) WHERE status = 'pending';

CREATE TRIGGER outbound_mail_touch_updated_at BEFORE UPDATE ON outbound_mail
    FOR EACH ROW EXECUTE FUNCTION touch_updated_at();
