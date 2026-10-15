-- Module: wholesale
-- Migration 0001: van sales — routes, stops, van loads and shop visits.

create schema wholesale;

create table wholesale.sales_routes (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid not null references core.branches (id),
  name text not null,
  -- The phone or tablet the sales representative sells from on the van.
  van_device_id uuid references core.devices (id) on delete set null,
  sales_representative_user_id uuid references core.users (id) on delete set null,
  vehicle_registration_number text,
  -- ISO day numbers the route runs: 1 = Monday ... 7 = Sunday.
  days_of_week integer[] not null default '{}' check (days_of_week <@ array[1, 2, 3, 4, 5, 6, 7]),
  is_active boolean not null default true,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('wholesale.sales_routes');
create index sales_routes_branch_index on wholesale.sales_routes (branch_id);
create index sales_routes_sales_representative_index on wholesale.sales_routes (sales_representative_user_id);

-- A shop (customer) visited on the route, in driving order.
create table wholesale.route_stops (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  sales_route_id uuid not null references wholesale.sales_routes (id) on delete cascade,
  customer_id uuid not null references customers.customers (id) on delete cascade,
  stop_order integer not null default 0,
  -- The shop's typical order, used to pre-fill the van sale: [{"product_id": "...", "quantity": 12}]
  usual_order jsonb not null default '[]'::jsonb,
  notes text,
  is_active boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('wholesale.route_stops');
create index route_stops_route_order_index on wholesale.route_stops (sales_route_id, stop_order);
create unique index route_stops_customer_unique_index on wholesale.route_stops (sales_route_id, customer_id) where deleted_at is null;

-- Stock put on a van for one day's route; the van is its own stock location.
create table wholesale.van_loads (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  sales_route_id uuid not null references wholesale.sales_routes (id),
  loaded_on date not null,
  stock_location_id uuid not null references inventory.stock_locations (id),
  sales_representative_user_id uuid references core.users (id),
  loaded_by_user_id uuid references core.users (id),
  status text not null default 'loaded' check (status in ('loaded', 'on_route', 'returned', 'reconciled')),
  expected_cash_amount numeric(18,2) not null default 0,
  cash_collected_amount numeric(18,2) not null default 0,
  reconciled_by_user_id uuid references core.users (id),
  reconciled_at timestamptz,
  notes text,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('wholesale.van_loads');
create index van_loads_route_date_index on wholesale.van_loads (sales_route_id, loaded_on);
create index van_loads_status_index on wholesale.van_loads (organization_id, status);

create table wholesale.van_load_lines (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  van_load_id uuid not null references wholesale.van_loads (id) on delete cascade,
  product_id uuid not null references catalog.products (id),
  quantity_loaded numeric(18,3) not null check (quantity_loaded >= 0),
  quantity_sold numeric(18,3) not null default 0 check (quantity_sold >= 0),
  quantity_returned numeric(18,3) not null default 0 check (quantity_returned >= 0),
  -- Damaged or missing on return; loaded − sold − returned − damaged should be zero after reconciliation.
  quantity_damaged numeric(18,3) not null default 0 check (quantity_damaged >= 0),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('wholesale.van_load_lines');
create unique index van_load_lines_product_unique_index on wholesale.van_load_lines (van_load_id, product_id) where deleted_at is null;

-- One stop at a shop, recorded offline on the van device and synced later.
create table wholesale.route_visits (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  route_stop_id uuid not null references wholesale.route_stops (id),
  van_load_id uuid references wholesale.van_loads (id) on delete set null,
  visited_by_user_id uuid references core.users (id),
  visited_at timestamptz not null,
  outcome text not null check (outcome in ('sold', 'no_order', 'closed', 'skipped')),
  sale_id uuid references point_of_sale.sales (id) on delete set null,
  -- Payment taken against the shop's earlier credit balance during this visit.
  collected_old_balance_amount numeric(18,2) not null default 0,
  latitude numeric(9,6),
  longitude numeric(9,6),
  notes text,
  -- When the van device uploaded this visit.
  synced_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('wholesale.route_visits');
create index route_visits_route_stop_index on wholesale.route_visits (route_stop_id, visited_at);
create index route_visits_van_load_index on wholesale.route_visits (van_load_id);
