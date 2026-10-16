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

-- Foreign keys to two tables named `patch_targets` in different schemas.
-- `app.patch_targets` is generated, so `target_id` uses its typed id;
-- `legacy.patch_targets` is created by tests/fixture-app/tests/postgres.rs
-- before this schema and is not generated, so `legacy_target_id` keeps `i64`.
CREATE TABLE app.patch_links (
    id BIGSERIAL PRIMARY KEY,
    target_id BIGINT NOT NULL REFERENCES app.patch_targets (id),
    legacy_target_id BIGINT REFERENCES legacy.patch_targets (id)
);


-- Enum types whose labels are mixed-case, upper-case, and spaced; each
-- variant keeps its exact label. `app.ticket_state` is schema-qualified and
-- gains a value in alter_schema.sql.
CREATE TYPE ticket_priority AS ENUM ('Low', 'MEDIUM', 'high', 'Very High');
CREATE TYPE app.ticket_state AS ENUM ('Open', 'CLOSED');

CREATE TABLE app.tickets (
    id BIGSERIAL PRIMARY KEY,
    priority TICKET_PRIORITY NOT NULL,
    state app.ticket_state,
    watch_levels ticket_priority[] NOT NULL DEFAULT '{}'
);

-- An enum primary key: key parameters and the keyset cursor use the enum.
CREATE TABLE priority_notes (
    priority ticket_priority PRIMARY KEY,
    note TEXT NOT NULL
);

-- Value-list columns: text columns a CHECK restricts to fixed values become
-- enums named after table and column, encoded as text with each exact value.
-- Both tables have a `status` column, and each gets its own enum.
CREATE TABLE app.shipments (
    id BIGSERIAL PRIMARY KEY,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'In Transit', 'DELIVERED')),
    carrier VARCHAR(20),
    CONSTRAINT shipments_carrier_known CHECK (carrier IN ('dhl', 'ups'))
);

CREATE TABLE refunds (
    id BIGSERIAL PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('requested', 'paid'))
);

-- Unique keys: the composite unique constraint gets
-- `get_by_organization_id_and_code`; the expression index and the partial
-- index (its condition is not a soft-delete condition) get no lookup.
CREATE TABLE app.branches (
    id BIGSERIAL PRIMARY KEY,
    organization_id UUID NOT NULL,
    code TEXT NOT NULL,
    name TEXT NOT NULL,
    UNIQUE (organization_id, code)
);
CREATE UNIQUE INDEX branches_lower_name ON app.branches (lower(name));
CREATE UNIQUE INDEX branches_open_name ON app.branches (name) WHERE code <> 'CLOSED';
