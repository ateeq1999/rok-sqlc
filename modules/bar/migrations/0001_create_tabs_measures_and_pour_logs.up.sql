-- Module: bar
-- Migration 0001: open tabs, tot measures and bottle pour checks.

create schema bar;

-- A running tab; the items live on sale_id, an open point_of_sale sale with status open_tab.
create table bar.bar_tabs (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid not null references core.branches (id),
  sale_id uuid not null references point_of_sale.sales (id),
  tab_name text not null,
  -- Free text table or seat: "Counter 3", "VIP 2".
  table_label text,
  opened_by_user_id uuid references core.users (id),
  guest_count integer check (guest_count is null or guest_count > 0),
  -- Staff are warned when the tab passes this amount.
  spending_limit_amount numeric(18,2),
  -- The guest left a card or ID at the counter as security.
  card_held boolean not null default false,
  status text not null default 'open' check (status in ('open', 'closed', 'transferred')),
  transferred_to_bar_tab_id uuid references bar.bar_tabs (id),
  -- Last time anything was added; used to flag tabs that may walk out.
  idle_since_at timestamptz not null default now(),
  opened_at timestamptz not null default now(),
  closed_at timestamptz,
  closed_by_user_id uuid references core.users (id),
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('bar.bar_tabs');
create index bar_tabs_open_index on bar.bar_tabs (branch_id, idle_since_at) where status = 'open' and deleted_at is null;
create unique index bar_tabs_sale_unique_index on bar.bar_tabs (sale_id) where deleted_at is null;

-- Sizes a spirit bottle is sold in: single tot, double tot, whole bottle.
create table bar.pour_measures (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  -- The bottle product the measure is poured from.
  product_id uuid not null references catalog.products (id) on delete cascade,
  measure_name text not null,
  volume_millilitres numeric(18,3) not null check (volume_millilitres > 0),
  measures_per_bottle numeric(18,3) not null check (measures_per_bottle > 0),
  price_amount numeric(18,2) not null check (price_amount >= 0),
  is_active boolean not null default true,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('bar.pour_measures');
create index pour_measures_product_index on bar.pour_measures (product_id);

-- One opened bottle: tots sold versus tots the bottle should have given.
create table bar.bottle_pour_logs (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid not null references core.branches (id),
  product_id uuid not null references catalog.products (id),
  stock_batch_id uuid references inventory.stock_batches (id),
  -- Sticker written on the bottle when opened, when batches are not tracked.
  bottle_label text,
  opened_at timestamptz not null default now(),
  finished_at timestamptz,
  measures_poured numeric(18,3) not null default 0,
  measures_expected numeric(18,3) not null,
  -- Bottle weight at the check, used to estimate what is left.
  closing_weight_grams numeric(18,3),
  -- Positive means fewer measures were rung up than poured (loss).
  variance_measures numeric(18,3),
  checked_by_user_id uuid references core.users (id),
  checked_at timestamptz,
  notes text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('bar.bottle_pour_logs');
create index bottle_pour_logs_product_index on bar.bottle_pour_logs (branch_id, product_id, opened_at);
