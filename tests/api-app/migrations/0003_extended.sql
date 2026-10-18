-- Ticket 08's routes. `tests/fixture_compile/` sets the conventions
-- (`tenant_id`, `revision`, `deleted_at`) and turns `POST /search` on for
-- every feature.
--
-- `documents` has them all: it is tenant-scoped (`tenant_id`), versioned
-- (`revision`, advanced by a trigger), soft-deleted (`deleted_at`), `admin`
-- only and owned through `owner_id`. `code` is unique per tenant, so it has
-- `GET /by-code/{value}`; `secret` is hidden, so its unique key has no
-- `by-` route. A document belongs to a label (`labels` has many, which
-- only an admin reads, in their tenant) and to at most one task (`tasks`
-- has one, embedded by `?include=document`).
CREATE TABLE documents (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    owner_id UUID NOT NULL,
    label_id BIGINT REFERENCES labels (id),
    task_id UUID UNIQUE REFERENCES tasks (id),
    code TEXT NOT NULL,
    title TEXT NOT NULL,
    secret TEXT UNIQUE,
    revision BIGINT NOT NULL DEFAULT 1,
    deleted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, code)
);

CREATE FUNCTION bump_revision() RETURNS trigger
LANGUAGE plpgsql AS $$
begin
  new.revision := old.revision + 1;
  return new;
end;
$$;

CREATE TRIGGER bump_revision BEFORE UPDATE ON documents
    FOR EACH ROW EXECUTE FUNCTION bump_revision();

-- Pairs owned `tasks` with public `labels`: from a label, a caller sees and
-- pairs only their own tasks.
CREATE TABLE task_labels (
    task_id UUID NOT NULL REFERENCES tasks (id),
    label_id BIGINT NOT NULL REFERENCES labels (id),
    PRIMARY KEY (task_id, label_id)
);
