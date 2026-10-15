-- Module: fashion
-- Migration 0001: layaway (lipa kidogo kidogo) and size/colour exchanges.

create schema fashion;

-- Goods put aside for a customer who pays in instalments. The held goods are the lines of sale_id,
-- kept open (unpaid) until the layaway completes; stock stays reserved until then.
create table fashion.layaways (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid not null references core.branches (id),
  layaway_number text,
  customer_id uuid not null references customers.customers (id),
  sale_id uuid not null references point_of_sale.sales (id),
  total_amount numeric(18,2) not null check (total_amount >= 0),
  deposit_amount numeric(18,2) not null default 0 check (deposit_amount >= 0),
  -- Deposit plus all later instalments.
  paid_amount numeric(18,2) not null default 0 check (paid_amount >= 0),
  due_on date not null,
  status text not null default 'active' check (status in ('active', 'completed', 'cancelled', 'expired')),
  -- Where the held goods are kept (shelf, bag number, back room).
  goods_held_location text,
  goods_collected_at timestamptz,
  -- Fee kept from payments when a layaway is cancelled or expires.
  cancellation_fee_amount numeric(18,2) not null default 0,
  created_by_user_id uuid references core.users (id),
  notes text,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('fashion.layaways');
create index layaways_customer_index on fashion.layaways (customer_id);
create index layaways_status_due_index on fashion.layaways (organization_id, status, due_on);
create index layaways_sale_index on fashion.layaways (sale_id);

create table fashion.layaway_payments (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  layaway_id uuid not null references fashion.layaways (id) on delete cascade,
  amount numeric(18,2) not null check (amount > 0),
  payment_method text not null check (payment_method in ('cash', 'mobile_money', 'card', 'bank_transfer')),
  -- M-Pesa, Tigo Pesa or Airtel Money transaction code, bank reference.
  payment_reference text,
  is_deposit boolean not null default false,
  sale_payment_id uuid references point_of_sale.sale_payments (id) on delete set null,
  received_by_user_id uuid references core.users (id),
  paid_at timestamptz not null default now(),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('fashion.layaway_payments');
create index layaway_payments_layaway_index on fashion.layaway_payments (layaway_id);

-- A return of goods swapped for other goods (different size or colour), linking both sales.
create table fashion.exchange_records (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid not null references core.branches (id),
  original_sale_id uuid not null references point_of_sale.sales (id),
  new_sale_id uuid references point_of_sale.sales (id),
  customer_id uuid references customers.customers (id) on delete set null,
  reason_type text not null default 'wrong_size'
    check (reason_type in ('wrong_size', 'wrong_colour', 'defect', 'changed_mind', 'gift', 'other')),
  reason text,
  -- Positive: customer paid more; negative: customer received change or store credit.
  price_difference_amount numeric(18,2) not null default 0,
  approved_by_user_id uuid references core.users (id),
  processed_by_user_id uuid references core.users (id),
  exchanged_at timestamptz not null default now(),
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('fashion.exchange_records');
create index exchange_records_original_sale_index on fashion.exchange_records (original_sale_id);
create index exchange_records_new_sale_index on fashion.exchange_records (new_sale_id);
