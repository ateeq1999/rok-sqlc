-- The fixture application's schema: one table per shape of API feature
-- (`tests/fixture_compile/` configures their `[api]` entries).

-- A UUID key made in Rust (`uuid_v7`), a unique column, checks that become
-- validation rules, and the list's `q` (`title`, `body`) and `sort`.
CREATE TABLE notes (
    id UUID PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 80),
    body TEXT,
    priority INT NOT NULL DEFAULT 0 CHECK (priority BETWEEN 0 AND 9),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- A composite key, one of its columns text, the other a foreign key.
CREATE TABLE members (
    team TEXT NOT NULL,
    note_id UUID NOT NULL REFERENCES notes (id),
    role TEXT NOT NULL DEFAULT 'viewer',
    PRIMARY KEY (team, note_id)
);

-- No key: only list and create. Served under `/audit-log`.
CREATE TABLE audit_events (
    action TEXT NOT NULL,
    detail TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- A key the database makes, and `remove` skipped.
CREATE TABLE labels (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);

-- A join table: no API feature.
CREATE TABLE note_labels (
    note_id UUID NOT NULL REFERENCES notes (id),
    label_id BIGINT NOT NULL REFERENCES labels (id),
    PRIMARY KEY (note_id, label_id)
);

-- No key until `0002_draft_keys.sql` adds one. Its scaffolded `service.rs`
-- is checked in, with an override, and survives that change.
CREATE TABLE drafts (
    title TEXT NOT NULL
);

-- Access, ownership and hidden columns (`tests/fixture_compile/`
-- configures them): `accounts` is `admin` only and hides `password_hash`;
-- `tasks` has the default access (`user`) and is owned through `owner_id`,
-- an account's typed id.
CREATE TABLE accounts (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL
);

CREATE TABLE tasks (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES accounts (id),
    title TEXT NOT NULL,
    done BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
