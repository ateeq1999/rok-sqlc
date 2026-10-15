-- Module: butchery
-- Migration 0001: carcass intake, standard cuts and yield tracking.
-- Weighed sales need no table here: point_of_sale.sale_lines.quantity holds kilograms for weighed products,
-- and counter scales are core.devices rows with device_type 'scale'.

create schema butchery;

-- A whole or half animal bought from a supplier or slaughterhouse.
create table butchery.carcasses (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid not null references core.branches (id),
  supplier_name text,
  animal_type text not null check (animal_type in ('beef', 'goat', 'sheep', 'chicken', 'pork', 'fish')),
  received_on date not null default current_date,
  weight_kilograms numeric(18,3) not null check (weight_kilograms > 0),
  cost_per_kilogram_amount numeric(18,2) not null default 0,
  total_cost_amount numeric(18,2) not null default 0,
  -- Ear tag or slaughterhouse stamp number.
  tag_number text,
  status text not null default 'hanging' check (status in ('hanging', 'cutting', 'finished')),
  received_by_user_id uuid references core.users (id),
  notes text,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('butchery.carcasses');
create index carcasses_branch_status_index on butchery.carcasses (branch_id, status, received_on);

-- A standard cut (steak, mince, ribs, steki) and the share of a carcass it normally yields.
create table butchery.cut_definitions (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  product_id uuid not null references catalog.products (id) on delete cascade,
  animal_type text not null check (animal_type in ('beef', 'goat', 'sheep', 'chicken', 'pork', 'fish')),
  expected_yield_percent numeric(6,3) not null check (expected_yield_percent >= 0 and expected_yield_percent <= 100),
  sort_order integer not null default 0,
  is_active boolean not null default true,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('butchery.cut_definitions');
create unique index cut_definitions_product_animal_unique_index on butchery.cut_definitions (product_id, animal_type) where deleted_at is null;

-- Weight of each cut, bones, fat and loss taken from a carcass; actual yield versus expected.
create table butchery.carcass_yield_entries (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  carcass_id uuid not null references butchery.carcasses (id) on delete cascade,
  cut_definition_id uuid references butchery.cut_definitions (id),
  -- Product put into stock; for cuts without a definition, or bones sold as a product.
  product_id uuid references catalog.products (id),
  weight_kilograms numeric(18,3) not null check (weight_kilograms >= 0),
  entry_type text not null check (entry_type in ('cut', 'bones_and_fat', 'trim_loss', 'sold')),
  -- For entry_type sold: a whole-carcass or quarter sold straight off the hook.
  sale_line_id uuid references point_of_sale.sale_lines (id) on delete set null,
  stock_location_id uuid references inventory.stock_locations (id),
  recorded_by_user_id uuid references core.users (id),
  recorded_at timestamptz not null default now(),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('butchery.carcass_yield_entries');
create index carcass_yield_entries_carcass_index on butchery.carcass_yield_entries (carcass_id);
create index carcass_yield_entries_cut_definition_index on butchery.carcass_yield_entries (cut_definition_id);
