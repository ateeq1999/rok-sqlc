-- Fixture tables appended to modules/inventory/schema.sql by tests/fixture_compile/.
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

-- Reshaped by alter_schema.sql's ALTER COLUMN changes: the key and `amount`
-- widen to BIGINT, `code` (a unique key) becomes TEXT, `amount` gains
-- NOT NULL, `label` loses its default, `note` loses NOT NULL, and `views`
-- gains a default.
CREATE TABLE meter_readings (
    id INTEGER PRIMARY KEY,
    code VARCHAR(16) UNIQUE,
    amount INTEGER,
    label TEXT NOT NULL DEFAULT 'unlabelled',
    note TEXT NOT NULL,
    views INTEGER NOT NULL
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

-- An enum primary key gets a typed id wrapping the enum (`PriorityNoteId`),
-- which key parameters, the keyset cursor, and the foreign key into it use.
CREATE TABLE priority_notes (
    priority ticket_priority PRIMARY KEY,
    note TEXT NOT NULL
);

CREATE TABLE priority_escalations (
    id BIGSERIAL PRIMARY KEY,
    priority ticket_priority NOT NULL REFERENCES priority_notes (priority)
);

-- Renamed to `contact_channel` by alter_schema.sql, so the enum is
-- `ContactChannel` and both columns use it.
CREATE TYPE ticket_channel AS ENUM ('email', 'Phone');

CREATE TABLE ticket_contacts (
    id BIGSERIAL PRIMARY KEY,
    channel ticket_channel NOT NULL,
    fallbacks ticket_channel[]
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

-- Managed columns: tests/fixture_compile/ generates the fixture with
-- `managed_columns = ["updated_at", "row_version"]`, and a trigger shaped like
-- the Rok ERP's `core.touch_row` maintains both on update.
CREATE TABLE touched_notes (
    id BIGSERIAL PRIMARY KEY,
    body TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    row_version BIGINT NOT NULL DEFAULT 1
);

CREATE FUNCTION touch_row() RETURNS trigger
LANGUAGE plpgsql AS $$
begin
  new.updated_at := clock_timestamp();
  new.row_version := old.row_version + 1;
  return new;
end;
$$;

CREATE TRIGGER touch_row BEFORE UPDATE ON touched_notes
    FOR EACH ROW EXECUTE FUNCTION touch_row();

-- Text keys: the typed id wraps `String`, so the generated functions borrow
-- it (`&CurrencyId`); a key struct with a text column is borrowed too.
CREATE TABLE currencies (
    code TEXT PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE currency_rates (
    currency_code TEXT NOT NULL REFERENCES currencies (code),
    day DATE NOT NULL,
    rate NUMERIC(12, 6) NOT NULL,
    PRIMARY KEY (currency_code, day)
);

-- Version column: tests/fixture_compile/ generates the fixture with
-- `version_column = "revision"`, so `update` takes the expected revision and
-- returns an `UpdateOutcome`. A `touch_row`-style trigger advances it.
-- With `deleted_at` as the soft-delete column, a deleted row counts as missing.
CREATE TABLE versioned_documents (
    id BIGSERIAL PRIMARY KEY,
    title TEXT NOT NULL,
    note TEXT,
    revision BIGINT NOT NULL DEFAULT 1,
    deleted_at TIMESTAMPTZ
);

CREATE FUNCTION bump_revision() RETURNS trigger
LANGUAGE plpgsql AS $$
begin
  new.revision := old.revision + 1;
  return new;
end;
$$;

CREATE TRIGGER bump_revision BEFORE UPDATE ON versioned_documents
    FOR EACH ROW EXECUTE FUNCTION bump_revision();

-- Soft delete: tests/fixture_compile/ generates the fixture with
-- `soft_delete_column = "deleted_at"`. The code index is unique among live
-- rows only, so it is a unique key and gets `get_by_code`.
CREATE TABLE soft_notes (
    id BIGSERIAL PRIMARY KEY,
    code TEXT NOT NULL,
    body TEXT NOT NULL,
    deleted_at TIMESTAMPTZ
);
CREATE UNIQUE INDEX soft_notes_code_unique_index ON soft_notes (code) WHERE deleted_at IS NULL;

-- Tenant scoping: tests/fixture_compile/ generates the fixture with
-- `tenant_column = "tenant_id"`. `tenant_notes` also has the soft-delete and
-- version columns, so every write path is scoped to the tenant.
CREATE TABLE tenant_notes (
    id BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL,
    code TEXT NOT NULL,
    body TEXT NOT NULL,
    revision BIGINT NOT NULL DEFAULT 1,
    deleted_at TIMESTAMPTZ,
    UNIQUE (tenant_id, code)
);

CREATE TRIGGER bump_revision BEFORE UPDATE ON tenant_notes
    FOR EACH ROW EXECUTE FUNCTION bump_revision();

-- The tenant column is part of the primary key, so the key is `code` alone.
CREATE TABLE tenant_members (
    tenant_id UUID NOT NULL,
    code TEXT NOT NULL,
    name TEXT NOT NULL,
    PRIMARY KEY (tenant_id, code)
);

-- Row-level security reading the tenant setting (`tenant_setting =
-- "app.tenant_id"`), as the Rok ERP's `core.prepare_table` sets up. An unset
-- (or reset, hence empty) setting matches no row.
CREATE TABLE rls_notes (
    id BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL,
    body TEXT NOT NULL
);
ALTER TABLE rls_notes ENABLE ROW LEVEL SECURITY;
CREATE POLICY rls_notes_tenant ON rls_notes
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

-- Relationships (ticket 20). `tenant_id` is the tenant column and references
-- `tenants`, which makes no relationship; `posts` has many `comments`, and
-- the loaders stay inside the tenant and skip soft-deleted rows. A comment's
-- `author_id` (ticket 22) points from a tenant table to the untenanted
-- `users`, so posts can be searched by their comments' authors.
CREATE TABLE tenants (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE users (
    id BIGSERIAL PRIMARY KEY,
    email TEXT NOT NULL
);

CREATE TABLE posts (
    id BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants (id),
    title TEXT NOT NULL,
    deleted_at TIMESTAMPTZ
);

CREATE TABLE comments (
    id BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants (id),
    post_id BIGINT NOT NULL REFERENCES posts (id),
    author_id BIGINT REFERENCES users (id),
    body TEXT NOT NULL,
    deleted_at TIMESTAMPTZ
);

-- Two foreign keys from `invoices` to `users`: the belongs-to names come from
-- the columns, and the inverses add them (`invoices_by_created_by_user`).
-- `user_profiles.user_id` is unique, so `users` has one `user_profile`.
CREATE TABLE invoices (
    id BIGSERIAL PRIMARY KEY,
    created_by_user_id BIGINT NOT NULL REFERENCES users (id),
    approved_by_user_id BIGINT REFERENCES users (id),
    total_cents BIGINT NOT NULL
);

CREATE TABLE user_profiles (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL UNIQUE REFERENCES users (id),
    bio TEXT NOT NULL
);

-- A self-reference through `parent_id`: `parent` and `children`.
CREATE TABLE org_units (
    id BIGSERIAL PRIMARY KEY,
    parent_id BIGINT REFERENCES org_units (id),
    name TEXT NOT NULL
);

-- Relationship configuration (ticket 24): tests/fixture_compile/ renames
-- `expense_claims.created_by_user` to `author` (and its inverse on `users` to
-- `authored_expense_claims`) and opts out `approved_by_user`, so neither side
-- of that foreign key gets a loader.
CREATE TABLE expense_claims (
    id BIGSERIAL PRIMARY KEY,
    created_by_user_id BIGINT NOT NULL REFERENCES users (id),
    approved_by_user_id BIGINT REFERENCES users (id),
    amount_cents BIGINT NOT NULL
);

-- Many-to-many (ticket 23). `role_permissions` is a join table: its two
-- foreign keys and the tenant form a unique key, so `roles` and `permissions`
-- relate many-to-many through it. It carries a payload column (`granted_at`)
-- and a soft-delete column, so `detach` soft-deletes and `attach` restores.
CREATE TABLE roles (
    id BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants (id),
    name TEXT NOT NULL
);

CREATE TABLE permissions (
    code TEXT PRIMARY KEY,
    description TEXT NOT NULL
);

CREATE TABLE role_permissions (
    id BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants (id),
    role_id BIGINT NOT NULL REFERENCES roles (id),
    permission_code TEXT NOT NULL REFERENCES permissions (code),
    granted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ,
    UNIQUE (tenant_id, role_id, permission_code)
);

-- A plain join table keyed by its pair: `detach` and `sync` delete rows.
CREATE TABLE member_groups (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE group_members (
    group_id BIGINT NOT NULL REFERENCES member_groups (id),
    user_id BIGINT NOT NULL REFERENCES users (id),
    PRIMARY KEY (group_id, user_id)
);

-- Column names near PostgreSQL's 63-byte limit (ticket 0.2-09): with a
-- `t_` or `r_` prefix, these 62-character names would be cut to the same
-- 63 bytes, so the joined read selects them under shortened unique aliases.
CREATE TABLE calibration_stations (
    id BIGSERIAL PRIMARY KEY,
    reading_taken_by_the_long_running_calibration_station_number_a TEXT NOT NULL,
    reading_taken_by_the_long_running_calibration_station_number_b TEXT NOT NULL
);

CREATE TABLE calibration_readings (
    id BIGSERIAL PRIMARY KEY,
    calibration_station_id BIGINT NOT NULL REFERENCES calibration_stations (id),
    reading_taken_by_the_long_running_calibration_station_number_a TEXT NOT NULL,
    reading_taken_by_the_long_running_calibration_station_number_b TEXT NOT NULL
);
