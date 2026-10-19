-- The pre-checks' scoping (0.5, ticket 05).
--
-- `bookmarks` has the default access (`user`), no tenant column and no
-- owner, and references `documents`, which is tenant-scoped and owned: the
-- `exists` check still takes the caller's tenant and id, so a document the
-- caller can't see counts as missing.
CREATE TABLE bookmarks (
    id UUID PRIMARY KEY,
    document_id UUID NOT NULL REFERENCES documents (id),
    note TEXT NOT NULL
);

-- `folders` is owned through `owner_id`, and a name is unique per owner:
-- the `unique` check takes the owner from the caller.
CREATE TABLE folders (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL,
    name TEXT NOT NULL,
    UNIQUE (owner_id, name)
);

-- `tags` is tenant-scoped, and a name is unique per tenant
-- ignoring case: a composite key with one `lower()` entry.
CREATE TABLE tags (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    name TEXT NOT NULL
);

CREATE UNIQUE INDEX tags_name_key ON tags (tenant_id, lower(name));
