-- A later migration, generated after extra_schema.sql by tests/fixture_compile.rs.
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
