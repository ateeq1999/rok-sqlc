-- Module: customers
-- Migration 0001: customer groups, customers, loyalty points and store credit accounts.

create schema customers;

-- Groups such as wholesale, staff or contractor, each optionally with its own price list.
create table customers.customer_groups (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  name text not null,
  description text,
  price_list_id uuid references catalog.price_lists (id),
  default_discount_percent numeric(6,3) not null default 0 check (default_discount_percent between 0 and 100),
  is_active boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('customers.customer_groups');
create unique index customer_groups_name_unique_index on customers.customer_groups (organization_id, lower(name)) where deleted_at is null;

create table customers.customers (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  customer_group_id uuid references customers.customer_groups (id),
  customer_type text not null default 'person' check (customer_type in ('person', 'business')),
  full_name text not null,
  -- Registered name when the customer is a business; full_name is then the contact person.
  business_name text,
  phone_number text,
  email text,
  address text,
  tax_identification_number text,
  vat_registration_number text,
  accepts_marketing_messages boolean not null default false,
  birthday_on date,
  -- Running total kept in step with loyalty_point_entries.
  loyalty_points_balance numeric(18,3) not null default 0,
  -- Branch where the customer was first registered.
  home_branch_id uuid references core.branches (id),
  notes text,
  is_active boolean not null default true,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('customers.customers');
create unique index customers_phone_number_unique_index on customers.customers (organization_id, phone_number) where phone_number is not null and deleted_at is null;
create index customers_email_index on customers.customers (organization_id, lower(email)) where email is not null;
create index customers_name_search_index on customers.customers using gin (to_tsvector('simple', full_name));
create index customers_group_index on customers.customers (customer_group_id);

-- How points are earned and what they are worth when spent.
create table customers.loyalty_programs (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  name text not null,
  -- points_per_amount points are earned for every earning_amount spent, e.g. 1 point per 1,000 TZS.
  points_per_amount numeric(18,3) not null default 1 check (points_per_amount >= 0),
  earning_amount numeric(18,2) not null default 1000 check (earning_amount > 0),
  -- Money value of one point when the customer pays with points.
  amount_per_point_redeemed numeric(18,2) not null default 10 check (amount_per_point_redeemed >= 0),
  minimum_points_to_redeem numeric(18,3) not null default 0,
  points_expire_after_days integer check (points_expire_after_days > 0),
  is_active boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('customers.loyalty_programs');

-- Append-only ledger of points earned, spent, expired or adjusted; never edited, so no deleted_at.
create table customers.loyalty_point_entries (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  customer_id uuid not null references customers.customers (id) on delete cascade,
  loyalty_program_id uuid references customers.loyalty_programs (id),
  entry_type text not null check (entry_type in ('earned', 'redeemed', 'expired', 'adjustment', 'reversed')),
  -- Positive adds points, negative removes them.
  points_change numeric(18,3) not null check (points_change <> 0),
  -- The document behind the entry, for example 'point_of_sale.sales' and its row id.
  source_table text,
  source_row_id uuid,
  expires_on date,
  reason text,
  recorded_by_user_id uuid references core.users (id),
  occurred_at timestamptz not null default now(),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  row_version bigint not null default 1
);
select core.prepare_table('customers.loyalty_point_entries');
create index loyalty_point_entries_customer_index on customers.loyalty_point_entries (customer_id, occurred_at desc);
create index loyalty_point_entries_source_index on customers.loyalty_point_entries (source_table, source_row_id);

-- Store credit (buy now, pay later) for a trusted customer.
create table customers.credit_accounts (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  customer_id uuid not null references customers.customers (id) on delete cascade,
  credit_limit numeric(18,2) not null default 0 check (credit_limit >= 0),
  -- Amount the customer currently owes, kept in step with credit_entries.
  balance numeric(18,2) not null default 0,
  payment_terms_days integer not null default 30 check (payment_terms_days >= 0),
  status text not null default 'active' check (status in ('active', 'on_hold', 'closed')),
  approved_by_user_id uuid references core.users (id),
  notes text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('customers.credit_accounts');
create unique index credit_accounts_customer_unique_index on customers.credit_accounts (customer_id) where deleted_at is null;

-- Append-only ledger of credit charges and payments; never edited, so no deleted_at.
create table customers.credit_entries (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  credit_account_id uuid not null references customers.credit_accounts (id) on delete cascade,
  entry_type text not null check (entry_type in ('sale_charge', 'payment', 'adjustment')),
  -- Positive increases what the customer owes (a sale on credit), negative reduces it (a payment).
  amount numeric(18,2) not null check (amount <> 0),
  -- When a charge must be paid by.
  due_on date,
  payment_method text check (payment_method in ('cash', 'card', 'mobile_money', 'bank_transfer', 'other')),
  payment_reference text,
  source_table text,
  source_row_id uuid,
  notes text,
  recorded_by_user_id uuid references core.users (id),
  occurred_at timestamptz not null default now(),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  row_version bigint not null default 1
);
select core.prepare_table('customers.credit_entries');
create index credit_entries_account_index on customers.credit_entries (credit_account_id, occurred_at desc);
create index credit_entries_due_index on customers.credit_entries (organization_id, due_on) where due_on is not null;
create index credit_entries_source_index on customers.credit_entries (source_table, source_row_id);
