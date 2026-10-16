CREATE TYPE product_status AS ENUM ('draft', 'active', 'archived');

CREATE TABLE categories (
    id         BIGSERIAL PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE products (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    category_id BIGINT NOT NULL REFERENCES categories (id),
    sku         TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    price       NUMERIC(12, 2) NOT NULL,
    status      product_status NOT NULL DEFAULT 'draft',
    tags        TEXT[] NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
