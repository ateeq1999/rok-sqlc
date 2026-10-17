-- Generated into tests/fixture-app/src/plain_db/ without serde derives:
-- SQLx's `PgInterval` and `PgTimeTz` implement no serde traits, so a run with
-- `derive = ["serde"]` rejects this table.

CREATE TABLE timed_badges (
    id BIGSERIAL PRIMARY KEY,
    span INTERVAL NOT NULL,
    breaks INTERVAL[],
    badge BYTEA NOT NULL UNIQUE,
    photos BYTEA[],
    opens TIME WITH TIME ZONE NOT NULL,
    closes TIMETZ,
    slots TIMETZ[]
);
