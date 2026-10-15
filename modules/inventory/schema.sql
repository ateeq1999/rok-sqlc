CREATE TABLE suppliers (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    contact_email TEXT,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE categories (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT
);

CREATE TABLE warehouses (
    id UUID PRIMARY KEY,
    code TEXT NOT NULL,
    region TEXT
);

CREATE TABLE products (
    id UUID PRIMARY KEY,
    sku TEXT NOT NULL,
    name TEXT NOT NULL,
    category_id BIGINT NOT NULL REFERENCES categories (id),
    supplier_id UUID REFERENCES suppliers (id),
    price_cents BIGINT NOT NULL DEFAULT 0,
    stock_on_hand INTEGER NOT NULL DEFAULT 0,
    reorder_level SMALLINT NOT NULL DEFAULT 10,
    weight_kg DOUBLE PRECISION,
    discontinued BOOLEAN NOT NULL DEFAULT FALSE,
    released_on DATE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE stock_levels (
    id BIGSERIAL PRIMARY KEY,
    product_id UUID NOT NULL REFERENCES products (id),
    warehouse_id UUID NOT NULL REFERENCES warehouses (id),
    quantity INTEGER NOT NULL DEFAULT 0,
    counted_at TIMESTAMP NOT NULL
);
