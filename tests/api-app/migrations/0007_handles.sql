-- The `unique` pre-check (0.5): `handles` is tenant-scoped and soft-deleted,
-- public, with a case-insensitive key (`lower(email)`), a key per tenant
-- (`nickname`) and a composite one per tenant (`region` and `seat`), each
-- holding across live rows only.
CREATE TABLE handles (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    email TEXT NOT NULL,
    nickname TEXT NOT NULL,
    region TEXT NOT NULL,
    seat INTEGER NOT NULL,
    deleted_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX handles_email_key ON handles (lower(email))
    WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX handles_nickname_key ON handles (tenant_id, nickname)
    WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX handles_seat_key ON handles (tenant_id, region, seat)
    WHERE deleted_at IS NULL;
