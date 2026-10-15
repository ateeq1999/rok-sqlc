-- Module: bakery
-- Migration 0001: recipes, daily production plans, baking batches and waste records.

create schema bakery;

create table bakery.recipes (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  -- The finished product this recipe makes (bread loaf, maandazi, cake).
  output_product_id uuid not null references catalog.products (id),
  name text not null,
  yield_quantity numeric(18,3) not null check (yield_quantity > 0),
  yield_unit_id uuid references catalog.units_of_measure (id),
  instructions text,
  -- Cached ingredient cost per output unit; recalculated when ingredient costs change.
  cost_per_unit_amount numeric(18,2),
  cost_calculated_at timestamptz,
  is_active boolean not null default true,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('bakery.recipes');
create index recipes_output_product_index on bakery.recipes (output_product_id);

create table bakery.recipe_ingredients (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  recipe_id uuid not null references bakery.recipes (id) on delete cascade,
  ingredient_product_id uuid not null references catalog.products (id),
  quantity numeric(18,3) not null check (quantity > 0),
  unit_id uuid references catalog.units_of_measure (id),
  -- Extra used up in handling (flour dust, trimmings), added on top of quantity.
  waste_percent numeric(6,3) not null default 0,
  sort_order integer not null default 0,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('bakery.recipe_ingredients');
create index recipe_ingredients_recipe_index on bakery.recipe_ingredients (recipe_id);
create index recipe_ingredients_ingredient_product_index on bakery.recipe_ingredients (ingredient_product_id);

-- What to bake for one day at one branch.
create table bakery.production_plans (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid not null references core.branches (id),
  plan_for_on date not null,
  status text not null default 'draft' check (status in ('draft', 'confirmed', 'baking', 'done')),
  created_by_user_id uuid references core.users (id),
  -- Quantities were proposed by Msaidizi from recent sales and leftovers.
  suggested_by_assistant boolean not null default false,
  notes text,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('bakery.production_plans');
create unique index production_plans_branch_day_unique_index on bakery.production_plans (branch_id, plan_for_on) where deleted_at is null;

create table bakery.production_plan_lines (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  production_plan_id uuid not null references bakery.production_plans (id) on delete cascade,
  recipe_id uuid not null references bakery.recipes (id),
  suggested_quantity numeric(18,3),
  planned_quantity numeric(18,3) not null check (planned_quantity >= 0),
  -- Already ordered in advance by customers (cakes, bulk bread for schools).
  preorder_quantity numeric(18,3) not null default 0 check (preorder_quantity >= 0),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('bakery.production_plan_lines');
create index production_plan_lines_plan_index on bakery.production_plan_lines (production_plan_id);

-- One actual bake: ingredients leave stock, finished goods enter stock.
create table bakery.production_batches (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  production_plan_line_id uuid references bakery.production_plan_lines (id) on delete set null,
  recipe_id uuid not null references bakery.recipes (id),
  stock_location_id uuid references inventory.stock_locations (id),
  -- Batch created for the finished goods, for date tracking.
  stock_batch_id uuid references inventory.stock_batches (id),
  quantity_made numeric(18,3) not null check (quantity_made >= 0),
  started_at timestamptz,
  finished_at timestamptz,
  baked_by_user_id uuid references core.users (id),
  ingredient_cost_amount numeric(18,2),
  notes text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('bakery.production_batches');
create index production_batches_plan_line_index on bakery.production_batches (production_plan_line_id);
create index production_batches_recipe_index on bakery.production_batches (recipe_id, finished_at);

create table bakery.waste_records (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid not null references core.branches (id),
  product_id uuid not null references catalog.products (id),
  production_batch_id uuid references bakery.production_batches (id) on delete set null,
  quantity numeric(18,3) not null check (quantity > 0),
  waste_reason text not null check (waste_reason in ('left_over', 'burnt', 'expired', 'damaged')),
  action_taken text not null check (action_taken in ('sold_day_old', 'staff_meal', 'donated', 'thrown_away')),
  cost_amount numeric(18,2),
  recorded_on date not null default current_date,
  recorded_by_user_id uuid references core.users (id),
  notes text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('bakery.waste_records');
create index waste_records_branch_day_index on bakery.waste_records (branch_id, recorded_on);
create index waste_records_product_index on bakery.waste_records (product_id);
