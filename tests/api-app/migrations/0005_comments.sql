-- The `exists` pre-check against a tenant-scoped, soft-deleted table (0.5):
-- a comment belongs to a live `documents` row of its tenant. `comments` has
-- the default access (`user`) and no owner.
CREATE TABLE comments (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    document_id UUID NOT NULL REFERENCES documents (id),
    body TEXT NOT NULL
);
