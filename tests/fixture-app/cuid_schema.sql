-- Generated into tests/fixture-app/src/cuid_db/ with
-- `primary_key_ids = "cuid2"`: `create` makes a CUID2 for a text key the
-- input leaves out, over any database default. A `UUID` key is left alone.

CREATE TABLE badges (
    id TEXT PRIMARY KEY DEFAULT 'not-a-cuid',
    label TEXT NOT NULL
);

CREATE TABLE sessions (
    id UUID PRIMARY KEY,
    badge_id TEXT NOT NULL REFERENCES badges (id)
);
