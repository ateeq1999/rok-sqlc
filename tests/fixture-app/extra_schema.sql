-- Fixture tables appended to modules/inventory/schema.sql by tests/fixture_compile.rs.
-- Each one targets a generator rule; tests/fixture-app/tests/postgres.rs exercises them.

-- Composite primary key, arrays, TIME, NUMERIC; no defaults, so `create` is static SQL.
CREATE TABLE feature_line_items (
    order_id UUID NOT NULL,
    product_id UUID NOT NULL,
    labels TEXT[] NOT NULL,
    weekdays INTEGER[],
    reporter_ids UUID[],
    cut_off TIME,
    unit_price NUMERIC(18,2) NOT NULL,
    PRIMARY KEY (order_id, product_id)
);

CREATE SCHEMA app;

-- Schema-qualified table with an identity key and one column of every update
-- shape (required, nullable, defaulted, nullable + defaulted, generated).
CREATE TABLE app.patch_targets (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    label TEXT NOT NULL,
    note TEXT,
    score INTEGER NOT NULL DEFAULT 0,
    tag TEXT DEFAULT 'none',
    doubled INTEGER GENERATED ALWAYS AS (score * 2) STORED
);

-- SQL reserved words, a Rust keyword, mixed case, and a leading digit.
CREATE TABLE "Reserved Words" (
    "select" BIGSERIAL PRIMARY KEY,
    "from" TEXT NOT NULL,
    "type" TEXT,
    "Mixed Case" INTEGER NOT NULL DEFAULT 1,
    "user" TEXT,
    "1st" BOOLEAN
);

-- No primary key: only create, list, and count are generated.
CREATE TABLE audit_log (
    event TEXT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Every insertable column is defaulted: an empty input inserts DEFAULT VALUES.
CREATE TABLE counters (
    id BIGSERIAL PRIMARY KEY,
    hits INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Only array columns and no primary key: no sort keys and no key to page by,
-- so neither search function (nor filter.rs) is generated.
CREATE TABLE tag_sets (
    labels TEXT[] NOT NULL,
    scores INTEGER[]
);

-- Changed by alter_schema.sql: `title` is renamed, `legacy` dropped, and
-- `body` added, so the model must carry only the final columns.
CREATE TABLE revised_notes (
    id BIGSERIAL PRIMARY KEY,
    title TEXT NOT NULL,
    legacy TEXT NOT NULL
);
