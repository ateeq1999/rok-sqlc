CREATE TABLE organizations (
    id   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL
);

CREATE TABLE invoices (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations (id),
    number          TEXT NOT NULL,
    total           NUMERIC(12, 2) NOT NULL,
    status          TEXT NOT NULL DEFAULT 'open'
                    CHECK (status IN ('open', 'paid', 'void')),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    row_version     BIGINT NOT NULL DEFAULT 1,
    deleted_at      TIMESTAMPTZ,
    UNIQUE (organization_id, number)
);
