-- A later migration, generated after extra_schema.sql by tests/fixture_compile/.
-- Its statements change or leave alone tables created earlier.

ALTER TABLE revised_notes ADD COLUMN body TEXT NOT NULL DEFAULT '';
ALTER TABLE revised_notes RENAME COLUMN title TO headline;
ALTER TABLE revised_notes DROP COLUMN legacy;

-- Ignored on purpose: none of these change the model.
ALTER TABLE revised_notes ENABLE ROW LEVEL SECURITY;
CREATE INDEX revised_notes_headline_index ON revised_notes (headline);
COMMENT ON TABLE revised_notes IS 'Notes whose shape migrations changed';

-- A value added to an enum type created earlier.
ALTER TYPE app.ticket_state ADD VALUE 'Archived' AFTER 'CLOSED';

-- An enum type renamed after a table started using it.
ALTER TYPE ticket_channel RENAME TO contact_channel;

-- A value-list column added by a later migration.
ALTER TABLE refunds ADD COLUMN reason TEXT;
ALTER TABLE refunds ADD CONSTRAINT refunds_reason_known CHECK (reason IN ('damaged', 'late'));

-- A unique key added by a later migration: `products` gains `get_by_sku`.
ALTER TABLE products ADD CONSTRAINT products_sku_key UNIQUE (sku);

-- Column changes: `meter_readings` ends up in its final shape, and its
-- primary key and unique key follow their columns' new types.
ALTER TABLE meter_readings ALTER COLUMN id TYPE BIGINT;
ALTER TABLE meter_readings ALTER COLUMN code SET DATA TYPE TEXT;
ALTER TABLE meter_readings
    ALTER COLUMN amount TYPE BIGINT USING amount::bigint,
    ALTER COLUMN amount SET NOT NULL;
ALTER TABLE meter_readings ALTER COLUMN label DROP DEFAULT;
ALTER TABLE meter_readings ALTER COLUMN note DROP NOT NULL;
ALTER TABLE meter_readings ALTER COLUMN views SET DEFAULT 0;
