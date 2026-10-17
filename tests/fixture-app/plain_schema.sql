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

-- `MONEY` and the range types are SQLx's own `PgMoney` and `PgRange<T>`,
-- which implement no serde traits either. The network types come from
-- SQLx's `ipnetwork` and `mac_address` features.
CREATE TABLE ledger_entries (
    id BIGSERIAL PRIMARY KEY,
    amount MONEY NOT NULL,
    seats INT4RANGE,
    quota INT8RANGE,
    rates NUMRANGE,
    valid_on DATERANGE,
    shift TSRANGE,
    open_at TSTZRANGE,
    client INET NOT NULL,
    subnet CIDR,
    device MACADDR
);

-- Generated with `primary_key_ids = "uuid_v7"`: `create` makes a UUID
-- version 7 when the input leaves the id out, over any database default.
CREATE TABLE ordered_events (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL
);
CREATE TABLE defaulted_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL
);
