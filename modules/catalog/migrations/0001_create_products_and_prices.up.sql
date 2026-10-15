-- Module: catalog
-- Migration 0001: products, variants, barcodes, units, taxes and price lists.

create schema catalog;

create table catalog.units_of_measure (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  name text not null,
  symbol text not null,
  measures text not null check (measures in ('count', 'weight', 'volume', 'length', 'time')),
  -- How many of the base unit one of this unit is: kilogram = 1000 grams.
  base_unit_id uuid references catalog.units_of_measure (id),
  conversion_factor numeric(18,6) not null default 1,
  allows_fractions boolean not null default false,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, symbol)
);
select core.prepare_table('catalog.units_of_measure');

create table catalog.tax_rates (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  name text not null,
  rate_percent numeric(6,3) not null,
  is_included_in_price boolean not null default true,
  fiscal_code text,
  is_default boolean not null default false,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('catalog.tax_rates');

create table catalog.categories (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  parent_category_id uuid references catalog.categories (id),
  name text not null,
  color text,
  sort_order integer not null default 0,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('catalog.categories');

create table catalog.products (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  category_id uuid references catalog.categories (id),
  name text not null,
  name_translations jsonb not null default '{}'::jsonb,
  description text,
  item_code text,
  product_type text not null default 'stocked'
    check (product_type in ('stocked', 'service', 'weighed', 'cut_to_length', 'bundle', 'made_to_order', 'gift_card')),
  selling_unit_id uuid references catalog.units_of_measure (id),
  tax_rate_id uuid references catalog.tax_rates (id),
  selling_price numeric(18,2) not null default 0,
  cost_price numeric(18,2) not null default 0,
  minimum_selling_price numeric(18,2),
  tracks_stock boolean not null default true,
  tracks_batches boolean not null default false,
  tracks_serial_numbers boolean not null default false,
  has_variants boolean not null default false,
  is_sold_online boolean not null default false,
  is_active boolean not null default true,
  image_attachment_id uuid references core.attachments (id),
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('catalog.products');
create unique index products_item_code_unique_index on catalog.products (organization_id, item_code) where item_code is not null and deleted_at is null;
create index products_name_search_index on catalog.products using gin (to_tsvector('simple', name));
create index products_category_index on catalog.products (category_id);

-- Size × colour for clothing, storage size for phones, and so on.
create table catalog.product_variants (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  product_id uuid not null references catalog.products (id) on delete cascade,
  -- {"size": "M", "colour": "Navy"}
  attributes jsonb not null default '{}'::jsonb,
  item_code text,
  selling_price numeric(18,2),
  cost_price numeric(18,2),
  is_active boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('catalog.product_variants');
create index product_variants_product_index on catalog.product_variants (product_id);

create table catalog.product_barcodes (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  product_id uuid not null references catalog.products (id) on delete cascade,
  product_variant_id uuid references catalog.product_variants (id) on delete cascade,
  barcode text not null,
  -- A case barcode can sell 24 units at once.
  units_per_scan numeric(18,3) not null default 1,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('catalog.product_barcodes');
create unique index product_barcodes_unique_index on catalog.product_barcodes (organization_id, barcode) where deleted_at is null;

create table catalog.price_lists (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  name text not null,
  -- Retail, wholesale, contractor, staff, happy hour ...
  applies_to text not null default 'everyone' check (applies_to in ('everyone', 'customer_group', 'channel', 'time_window')),
  conditions jsonb not null default '{}'::jsonb,
  priority integer not null default 0,
  valid_from timestamptz,
  valid_until timestamptz,
  is_active boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('catalog.price_lists');

-- Quantity tiers: 1–4 at 62,500, 5–19 at 61,000, 20+ at 59,500.
create table catalog.price_list_items (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  price_list_id uuid not null references catalog.price_lists (id) on delete cascade,
  product_id uuid not null references catalog.products (id) on delete cascade,
  product_variant_id uuid references catalog.product_variants (id) on delete cascade,
  minimum_quantity numeric(18,3) not null default 1,
  unit_price numeric(18,2),
  discount_percent numeric(6,3),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (unit_price is not null or discount_percent is not null)
);
select core.prepare_table('catalog.price_list_items');
create index price_list_items_lookup_index on catalog.price_list_items (price_list_id, product_id, minimum_quantity);
