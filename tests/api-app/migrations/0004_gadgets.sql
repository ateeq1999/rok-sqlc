-- The column types the OpenAPI schemas describe (milestone 2): an enum type
-- and a value-list column, each keeping its exact labels, a decimal, JSON,
-- a network address and a date. `gadgets` is public
-- (`tests/fixture_compile/` configures it).
CREATE TYPE gadget_mood AS ENUM ('happy', 'so-so');

CREATE TABLE gadgets (
    id UUID PRIMARY KEY,
    mood gadget_mood NOT NULL,
    size TEXT NOT NULL CHECK (size IN ('small', 'Large')),
    price NUMERIC(10, 2),
    specs JSONB,
    host INET,
    made_on DATE
);
