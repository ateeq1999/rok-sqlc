-- Module: inventory
-- Migration 0002: transfers between locations, stock counts and reorder rules.

-- Moving stock from one location to another, for example warehouse to branch or store to van.
create table inventory.stock_transfers (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  transfer_number text not null,
  from_stock_location_id uuid not null references inventory.stock_locations (id),
  to_stock_location_id uuid not null references inventory.stock_locations (id),
  status text not null default 'draft'
    check (status in ('draft', 'sent', 'received', 'cancelled')),
  requested_by_user_id uuid references core.users (id),
  sent_by_user_id uuid references core.users (id),
  sent_at timestamptz,
  received_by_user_id uuid references core.users (id),
  received_at timestamptz,
  notes text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, transfer_number),
  check (from_stock_location_id <> to_stock_location_id)
);
select core.prepare_table('inventory.stock_transfers');
create index stock_transfers_status_index on inventory.stock_transfers (organization_id, status);

create table inventory.stock_transfer_lines (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  stock_transfer_id uuid not null references inventory.stock_transfers (id) on delete cascade,
  product_id uuid not null references catalog.products (id),
  product_variant_id uuid references catalog.product_variants (id),
  stock_batch_id uuid references inventory.stock_batches (id),
  stock_serial_number_id uuid references inventory.stock_serial_numbers (id),
  quantity_sent numeric(18,3) not null check (quantity_sent > 0),
  -- Filled in at the receiving end; a shortfall shows what went missing on the way.
  quantity_received numeric(18,3) check (quantity_received >= 0),
  unit_cost numeric(18,2),
  notes text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('inventory.stock_transfer_lines');
create index stock_transfer_lines_transfer_index on inventory.stock_transfer_lines (stock_transfer_id);

-- A full or partial stocktake of one location.
create table inventory.stock_counts (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  count_number text not null,
  stock_location_id uuid not null references inventory.stock_locations (id),
  count_scope text not null default 'full'
    check (count_scope in ('full', 'partial', 'spot_check')),
  status text not null default 'draft'
    check (status in ('draft', 'in_progress', 'completed', 'cancelled')),
  -- When the count is completed, differences become count_adjustment stock movements.
  started_by_user_id uuid references core.users (id),
  started_at timestamptz,
  completed_at timestamptz,
  approved_by_user_id uuid references core.users (id),
  notes text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, count_number)
);
select core.prepare_table('inventory.stock_counts');
create index stock_counts_location_index on inventory.stock_counts (stock_location_id);

create table inventory.stock_count_lines (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  stock_count_id uuid not null references inventory.stock_counts (id) on delete cascade,
  product_id uuid not null references catalog.products (id),
  product_variant_id uuid references catalog.product_variants (id),
  stock_batch_id uuid references inventory.stock_batches (id),
  -- What the system expected when counting started.
  expected_quantity numeric(18,3) not null default 0,
  -- Null until someone has counted this line.
  counted_quantity numeric(18,3) check (counted_quantity >= 0),
  difference_quantity numeric(18,3) generated always as (counted_quantity - expected_quantity) stored,
  unit_cost numeric(18,2),
  counted_by_user_id uuid references core.users (id),
  counted_at timestamptz,
  notes text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('inventory.stock_count_lines');
create index stock_count_lines_count_index on inventory.stock_count_lines (stock_count_id);

-- When to reorder a product at a location, used for low-stock alerts and suggested purchase orders.
create table inventory.reorder_rules (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  product_id uuid not null references catalog.products (id) on delete cascade,
  product_variant_id uuid references catalog.product_variants (id) on delete cascade,
  stock_location_id uuid not null references inventory.stock_locations (id) on delete cascade,
  -- Reorder when available quantity falls to or below this.
  minimum_quantity numeric(18,3) not null default 0 check (minimum_quantity >= 0),
  reorder_quantity numeric(18,3) not null default 0 check (reorder_quantity >= 0),
  -- Alternative to fixed quantities: order enough to cover this many days of recent sales.
  preferred_days_of_cover integer check (preferred_days_of_cover > 0),
  is_active boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('inventory.reorder_rules');
create unique index reorder_rules_unique_index on inventory.reorder_rules (
  product_id,
  coalesce(product_variant_id, '00000000-0000-0000-0000-000000000000'::uuid),
  stock_location_id
) where deleted_at is null;
