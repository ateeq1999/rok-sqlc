-- The `rok` preset expects the database to maintain `updated_at` and
-- `row_version` (managed columns), as this trigger does.
CREATE FUNCTION touch_row() RETURNS trigger AS $$
BEGIN
    NEW.updated_at := now();
    NEW.row_version := OLD.row_version + 1;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER invoices_touch_row BEFORE UPDATE ON invoices
    FOR EACH ROW EXECUTE FUNCTION touch_row();
