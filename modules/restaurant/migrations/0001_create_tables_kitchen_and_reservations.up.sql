-- Module: restaurant (also used by cafes)
-- Migration 0001: floor plan, modifiers, kitchen tickets, reservations and bill splitting.

create schema restaurant;

-- A room or zone on the floor plan: inside, terrace, rooftop, bar counter.
create table restaurant.dining_areas (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid not null references core.branches (id) on delete cascade,
  name text not null,
  sort_order integer not null default 0,
  is_active boolean not null default true,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('restaurant.dining_areas');
create index dining_areas_branch_index on restaurant.dining_areas (branch_id);

create table restaurant.dining_tables (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  dining_area_id uuid not null references restaurant.dining_areas (id) on delete cascade,
  name text not null,
  seats integer not null default 4 check (seats > 0),
  shape text not null default 'square' check (shape in ('square', 'round', 'rectangle', 'bar_stool', 'booth')),
  -- Position on the floor plan canvas, in canvas units from the top left corner.
  position_x numeric(10,2) not null default 0,
  position_y numeric(10,2) not null default 0,
  status text not null default 'free'
    check (status in ('free', 'seated', 'ordered', 'waiting_for_bill', 'needs_cleaning')),
  -- The open sale (bill) currently running on this table, if any.
  current_sale_id uuid references point_of_sale.sales (id) on delete set null,
  is_active boolean not null default true,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('restaurant.dining_tables');
create index dining_tables_dining_area_index on restaurant.dining_tables (dining_area_id);
create unique index dining_tables_name_unique_index on restaurant.dining_tables (dining_area_id, name) where deleted_at is null;
create index dining_tables_current_sale_index on restaurant.dining_tables (current_sale_id) where current_sale_id is not null;

-- "Choose your side", "Extra toppings": a set of options with a minimum and maximum pick count.
create table restaurant.modifier_groups (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  name text not null,
  name_translations jsonb not null default '{}'::jsonb,
  minimum_choices integer not null default 0 check (minimum_choices >= 0),
  maximum_choices integer check (maximum_choices is null or maximum_choices >= 1),
  sort_order integer not null default 0,
  is_active boolean not null default true,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (maximum_choices is null or maximum_choices >= minimum_choices)
);
select core.prepare_table('restaurant.modifier_groups');

create table restaurant.modifiers (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  modifier_group_id uuid not null references restaurant.modifier_groups (id) on delete cascade,
  name text not null,
  name_translations jsonb not null default '{}'::jsonb,
  -- Added to (or, if negative, taken off) the item price: "Extra cheese +2,000".
  price_change_amount numeric(18,2) not null default 0,
  -- Optional stocked product used up when this modifier is chosen (an extra egg, a sachet of sauce).
  stock_product_id uuid references catalog.products (id),
  stock_quantity_used numeric(18,3),
  is_default_choice boolean not null default false,
  sort_order integer not null default 0,
  is_active boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('restaurant.modifiers');
create index modifiers_modifier_group_index on restaurant.modifiers (modifier_group_id);

-- Which modifier groups appear when a menu item is ordered.
create table restaurant.product_modifier_groups (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  product_id uuid not null references catalog.products (id) on delete cascade,
  modifier_group_id uuid not null references restaurant.modifier_groups (id) on delete cascade,
  sort_order integer not null default 0,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('restaurant.product_modifier_groups');
create unique index product_modifier_groups_unique_index on restaurant.product_modifier_groups (product_id, modifier_group_id) where deleted_at is null;
create index product_modifier_groups_modifier_group_index on restaurant.product_modifier_groups (modifier_group_id);

-- Grill, cold kitchen, barista, pizza oven: where tickets are routed.
create table restaurant.kitchen_stations (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid not null references core.branches (id) on delete cascade,
  name text not null,
  -- Kitchen display screen or ticket printer for this station.
  device_id uuid references core.devices (id) on delete set null,
  -- Categories whose items are sent to this station.
  category_ids uuid[] not null default '{}',
  sort_order integer not null default 0,
  is_active boolean not null default true,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('restaurant.kitchen_stations');
create index kitchen_stations_branch_index on restaurant.kitchen_stations (branch_id);

create table restaurant.kitchen_tickets (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  ticket_number text,
  sale_id uuid not null references point_of_sale.sales (id) on delete cascade,
  dining_table_id uuid references restaurant.dining_tables (id) on delete set null,
  kitchen_station_id uuid references restaurant.kitchen_stations (id) on delete set null,
  -- 1 = starters, 2 = mains, 3 = dessert; later courses are held until fired.
  course_number integer not null default 1 check (course_number >= 1),
  status text not null default 'new' check (status in ('new', 'cooking', 'ready', 'served', 'cancelled')),
  -- Cafe counter orders: the name called out when the order is ready.
  name_on_order text,
  sent_by_user_id uuid references core.users (id),
  fired_at timestamptz,
  ready_at timestamptz,
  served_at timestamptz,
  notes text,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('restaurant.kitchen_tickets');
create index kitchen_tickets_sale_index on restaurant.kitchen_tickets (sale_id);
create index kitchen_tickets_station_status_index on restaurant.kitchen_tickets (kitchen_station_id, status, fired_at);
create index kitchen_tickets_dining_table_index on restaurant.kitchen_tickets (dining_table_id);

create table restaurant.kitchen_ticket_items (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  kitchen_ticket_id uuid not null references restaurant.kitchen_tickets (id) on delete cascade,
  sale_line_id uuid not null references point_of_sale.sale_lines (id) on delete cascade,
  quantity numeric(18,3) not null check (quantity > 0),
  -- Chosen modifiers as shown to the cook: [{"modifier_id": "...", "name": "No onions"}]
  modifiers jsonb not null default '[]'::jsonb,
  notes text,
  status text not null default 'new' check (status in ('new', 'cooking', 'ready', 'served', 'cancelled')),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('restaurant.kitchen_ticket_items');
create index kitchen_ticket_items_kitchen_ticket_index on restaurant.kitchen_ticket_items (kitchen_ticket_id);
create index kitchen_ticket_items_sale_line_index on restaurant.kitchen_ticket_items (sale_line_id);

create table restaurant.reservations (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid not null references core.branches (id) on delete cascade,
  customer_id uuid references customers.customers (id) on delete set null,
  -- For guests who are not saved customers.
  guest_name text,
  guest_phone_number text,
  party_size integer not null check (party_size > 0),
  reserved_for_at timestamptz not null,
  dining_table_id uuid references restaurant.dining_tables (id) on delete set null,
  status text not null default 'booked' check (status in ('booked', 'arrived', 'seated', 'no_show', 'cancelled')),
  deposit_amount numeric(18,2) not null default 0,
  source text not null default 'phone' check (source in ('phone', 'walk_in', 'online')),
  booked_by_user_id uuid references core.users (id),
  notes text,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('restaurant.reservations');
create index reservations_branch_time_index on restaurant.reservations (branch_id, reserved_for_at);
create index reservations_customer_index on restaurant.reservations (customer_id);

-- One part of a bill that guests pay separately.
create table restaurant.bill_splits (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  sale_id uuid not null references point_of_sale.sales (id) on delete cascade,
  split_method text not null check (split_method in ('by_item', 'equal', 'custom_amount')),
  part_number integer not null check (part_number >= 1),
  amount numeric(18,2) not null check (amount >= 0),
  -- For by_item splits: the sale lines this part covers.
  sale_line_ids uuid[] not null default '{}',
  paid_sale_payment_id uuid references point_of_sale.sale_payments (id) on delete set null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('restaurant.bill_splits');
create unique index bill_splits_part_unique_index on restaurant.bill_splits (sale_id, part_number) where deleted_at is null;
