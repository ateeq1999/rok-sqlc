-- Module: inventory
-- Migration 0001: stock locations, batches, serial numbers, stock levels and the stock movement ledger.

create schema inventory;

-- Places stock is kept: shop floor shelves, back store, warehouse, delivery van.
create table inventory.stock_locations (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  -- Null for a location not tied to one branch, such as a central warehouse.
  branch_id uuid references core.branches (id),
  parent_location_id uuid references inventory.stock_locations (id),
  name text not null,
  code text,
  location_type text not null default 'shop_floor'
    check (location_type in ('shop_floor', 'back_store', 'warehouse', 'van', 'display', 'other')),
  -- Whether the register may sell directly from this location.
  is_sellable_from boolean not null default true,
  -- Staff member responsible, for example the van driver.
  responsible_user_id uuid references core.users (id),
  is_active boolean not null default true,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('inventory.stock_locations');
create unique index stock_locations_code_unique_index on inventory.stock_locations (organization_id, code) where code is not null and deleted_at is null;
create index stock_locations_branch_index on inventory.stock_locations (branch_id);

-- A lot of one product received together, tracked for expiry (medicine, food, cosmetics).
create table inventory.stock_batches (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  product_id uuid not null references catalog.products (id),
  product_variant_id uuid references catalog.product_variants (id),
  batch_number text not null,
  manufactured_on date,
  expires_on date,
  received_on date,
  notes text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('inventory.stock_batches');
create unique index stock_batches_number_unique_index on inventory.stock_batches (
  organization_id, product_id,
  coalesce(product_variant_id, '00000000-0000-0000-0000-000000000000'::uuid),
  batch_number
) where deleted_at is null;
create index stock_batches_expiry_index on inventory.stock_batches (organization_id, expires_on) where expires_on is not null;

-- One physical unit tracked individually (phones, laptops, appliances).
create table inventory.stock_serial_numbers (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  product_id uuid not null references catalog.products (id),
  product_variant_id uuid references catalog.product_variants (id),
  serial_number text not null,
  -- Second identifier on the same unit, for example the second IMEI of a dual-SIM phone.
  secondary_serial_number text,
  stock_batch_id uuid references inventory.stock_batches (id),
  -- Where the unit is now; null once it has left the business.
  stock_location_id uuid references inventory.stock_locations (id),
  status text not null default 'in_stock'
    check (status in ('in_stock', 'sold', 'returned', 'in_repair', 'written_off')),
  unit_cost numeric(18,2),
  received_on date,
  -- point_of_sale.sales id; no foreign key because point_of_sale depends on inventory, not the reverse.
  sold_on_sale_id uuid,
  sold_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('inventory.stock_serial_numbers');
create unique index stock_serial_numbers_unique_index on inventory.stock_serial_numbers (organization_id, product_id, serial_number) where deleted_at is null;
create index stock_serial_numbers_lookup_index on inventory.stock_serial_numbers (organization_id, serial_number);
create index stock_serial_numbers_secondary_lookup_index on inventory.stock_serial_numbers (organization_id, secondary_serial_number) where secondary_serial_number is not null;
create index stock_serial_numbers_sale_index on inventory.stock_serial_numbers (sold_on_sale_id) where sold_on_sale_id is not null;

-- Current quantity of each product (and variant) at each location, kept in step with stock_movements.
create table inventory.stock_levels (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  product_id uuid not null references catalog.products (id) on delete cascade,
  product_variant_id uuid references catalog.product_variants (id) on delete cascade,
  stock_location_id uuid not null references inventory.stock_locations (id) on delete cascade,
  quantity_on_hand numeric(18,3) not null default 0,
  -- Held for parked sales, online orders and open transfers; not available to sell.
  quantity_reserved numeric(18,3) not null default 0 check (quantity_reserved >= 0),
  -- Weighted average cost per unit, used for margin and stock value.
  average_cost numeric(18,2) not null default 0,
  last_movement_at timestamptz,
  last_counted_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('inventory.stock_levels');
create unique index stock_levels_unique_index on inventory.stock_levels (
  product_id,
  coalesce(product_variant_id, '00000000-0000-0000-0000-000000000000'::uuid),
  stock_location_id
);
create index stock_levels_location_index on inventory.stock_levels (stock_location_id);

-- Append-only ledger: every change to stock is one row here. Rows are never edited or deleted,
-- corrections are new rows, so there is no deleted_at column.
create table inventory.stock_movements (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  product_id uuid not null references catalog.products (id),
  product_variant_id uuid references catalog.product_variants (id),
  stock_location_id uuid not null references inventory.stock_locations (id),
  movement_type text not null
    check (movement_type in ('sale', 'return', 'purchase_receipt', 'transfer_out', 'transfer_in', 'count_adjustment', 'waste', 'production_use', 'production_output', 'opening_balance')),
  -- Positive adds stock, negative removes it.
  quantity_change numeric(18,3) not null check (quantity_change <> 0),
  unit_cost numeric(18,2),
  stock_batch_id uuid references inventory.stock_batches (id),
  stock_serial_number_id uuid references inventory.stock_serial_numbers (id),
  -- The document that caused the movement, for example 'point_of_sale.sale_lines' and its row id.
  source_table text,
  source_row_id uuid,
  reason text,
  recorded_by_user_id uuid references core.users (id),
  device_id uuid references core.devices (id),
  -- When it happened on the till, which can be earlier than created_at for offline devices.
  occurred_at timestamptz not null default now(),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  row_version bigint not null default 1
);
select core.prepare_table('inventory.stock_movements');
create index stock_movements_product_index on inventory.stock_movements (product_id, product_variant_id, stock_location_id, occurred_at);
create index stock_movements_organization_time_index on inventory.stock_movements (organization_id, occurred_at desc);
create index stock_movements_source_index on inventory.stock_movements (source_table, source_row_id);
create index stock_movements_batch_index on inventory.stock_movements (stock_batch_id) where stock_batch_id is not null;
create index stock_movements_serial_number_index on inventory.stock_movements (stock_serial_number_id) where stock_serial_number_id is not null;
