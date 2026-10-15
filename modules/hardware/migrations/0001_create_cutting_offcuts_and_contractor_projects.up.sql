-- Module: hardware
-- Migration 0001: cut-to-length selling (pipes, cable, chain, timber), offcuts and contractor projects.

create schema hardware;

-- How a product sold by length is cut from stock lengths and priced.
create table hardware.cut_to_length_rules (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  product_id uuid not null references catalog.products (id) on delete cascade,
  -- Length of one full stock piece or roll, e.g. 6 metres of PVC pipe, 100 metres of cable.
  stock_length_metres numeric(18,3) not null check (stock_length_metres > 0),
  minimum_cut_metres numeric(18,3) not null default 0 check (minimum_cut_metres >= 0),
  -- Charged per cut on top of the length price.
  cutting_fee_amount numeric(18,2) not null default 0,
  -- Offcuts sell at this percent of the normal per-metre price.
  offcut_price_percent numeric(6,3) not null default 100,
  -- Pieces shorter than this are scrapped, not kept as offcuts.
  offcut_minimum_metres numeric(18,3) not null default 0,
  is_active boolean not null default true,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('hardware.cut_to_length_rules');
create unique index cut_to_length_rules_product_unique_index on hardware.cut_to_length_rules (product_id) where deleted_at is null;

-- A leftover piece from a cut, kept to sell at a lower price.
create table hardware.offcuts (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  product_id uuid not null references catalog.products (id),
  length_metres numeric(18,3) not null check (length_metres > 0),
  -- inventory.stock_locations id; no foreign key because hardware does not depend on the inventory module.
  stock_location_reference uuid,
  -- The sale line whose cut produced this offcut.
  source_sale_line_id uuid references point_of_sale.sale_lines (id) on delete set null,
  sold_sale_line_id uuid references point_of_sale.sale_lines (id) on delete set null,
  price_amount numeric(18,2),
  status text not null default 'available' check (status in ('available', 'sold', 'scrapped')),
  label text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('hardware.offcuts');
create index offcuts_available_index on hardware.offcuts (product_id, length_metres) where status = 'available' and deleted_at is null;

-- A building job a fundi or contractor buys materials for, often on credit and from a quotation.
create table hardware.contractor_projects (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  -- The contractor (fundi) buying for this project.
  customer_id uuid not null references customers.customers (id),
  project_name text not null,
  site_address text,
  site_contact_phone_number text,
  quotation_id uuid references quotes_invoices.quotations (id) on delete set null,
  trade_discount_percent numeric(6,3) not null default 0,
  -- Agreed credit terms in words; the enforced limit lives in customers.credit_accounts.
  credit_terms_note text,
  budget_amount numeric(18,2),
  started_on date,
  expected_completion_on date,
  status text not null default 'active' check (status in ('planned', 'active', 'on_hold', 'completed', 'cancelled')),
  notes text,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('hardware.contractor_projects');
create index contractor_projects_customer_index on hardware.contractor_projects (customer_id);
create index contractor_projects_status_index on hardware.contractor_projects (organization_id, status);
