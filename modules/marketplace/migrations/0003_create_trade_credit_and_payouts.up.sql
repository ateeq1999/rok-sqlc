-- Module: marketplace
-- Migration 0003: trade credit for buyers (buyer-owned rows) and payouts to vendors (vendor-owned rows).
-- Both use the standard organization_isolation policy only.

-- "Buy now, pay later" limit for a buying shop, funded by a lending partner.
create table marketplace.trade_credit_accounts (
  id uuid primary key default core.generate_uuid_v7(),
  -- The buyer's organization.
  organization_id uuid not null references core.organizations (id) on delete cascade,
  credit_limit_amount numeric(18,2) not null default 0 check (credit_limit_amount >= 0),
  used_amount numeric(18,2) not null default 0 check (used_amount >= 0),
  fee_percent_per_thirty_days numeric(6,3) not null default 0,
  -- Bills repaid within this many days carry no fee.
  fee_free_days integer not null default 0 check (fee_free_days >= 0),
  status text not null default 'applied'
    check (status in ('applied', 'active', 'paused', 'closed', 'declined')),
  -- The lending partner's account reference.
  lender_reference text,
  repayment_method text not null default 'on_due_date' check (repayment_method in ('auto_from_takings', 'on_due_date')),
  -- Share of daily takings swept to repay open bills when repayment_method = auto_from_takings.
  auto_repayment_percent numeric(6,3) check (auto_repayment_percent between 0 and 100),
  next_review_on date,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (repayment_method <> 'auto_from_takings' or auto_repayment_percent is not null)
);
select core.prepare_table('marketplace.trade_credit_accounts');
-- One open account per business.
create unique index trade_credit_accounts_open_unique_index on marketplace.trade_credit_accounts (organization_id)
  where deleted_at is null and status in ('applied', 'active', 'paused');

-- What the buyer owes for one order (or a whole checkout group) paid with trade credit.
create table marketplace.trade_credit_bills (
  id uuid primary key default core.generate_uuid_v7(),
  -- The buyer's organization.
  organization_id uuid not null references core.organizations (id) on delete cascade,
  trade_credit_account_id uuid not null references marketplace.trade_credit_accounts (id),
  order_id uuid references marketplace.orders (id),
  group_number text,
  principal_amount numeric(18,2) not null check (principal_amount >= 0),
  fee_amount numeric(18,2) not null default 0 check (fee_amount >= 0),
  issued_on date not null default current_date,
  due_on date not null,
  paid_amount numeric(18,2) not null default 0 check (paid_amount >= 0),
  status text not null default 'open' check (status in ('open', 'paid', 'overdue', 'written_off')),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (order_id is not null or group_number is not null),
  check (due_on >= issued_on)
);
select core.prepare_table('marketplace.trade_credit_bills');
create index trade_credit_bills_account_index on marketplace.trade_credit_bills (trade_credit_account_id, due_on);
create index trade_credit_bills_open_index on marketplace.trade_credit_bills (organization_id, due_on) where status in ('open', 'overdue');
create index trade_credit_bills_order_index on marketplace.trade_credit_bills (order_id) where order_id is not null;

create table marketplace.trade_credit_repayments (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  trade_credit_bill_id uuid not null references marketplace.trade_credit_bills (id),
  amount numeric(18,2) not null check (amount > 0),
  method text not null check (method in ('auto_from_takings', 'mobile_money', 'bank_transfer', 'cash')),
  -- Mobile money or bank transaction reference.
  reference text,
  paid_at timestamptz not null default now(),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('marketplace.trade_credit_repayments');
create index trade_credit_repayments_bill_index on marketplace.trade_credit_repayments (trade_credit_bill_id, paid_at);

-- Money the platform pays a vendor for a period, after commission and adjustments.
create table marketplace.payouts (
  id uuid primary key default core.generate_uuid_v7(),
  -- The vendor's organization.
  organization_id uuid not null references core.organizations (id) on delete cascade,
  vendor_id uuid not null references marketplace.vendors (id),
  period_start_on date not null,
  period_end_on date not null,
  gross_amount numeric(18,2) not null default 0,
  commission_amount numeric(18,2) not null default 0,
  adjustments_amount numeric(18,2) not null default 0,
  net_amount numeric(18,2) not null default 0,
  status text not null default 'scheduled' check (status in ('scheduled', 'paid', 'failed')),
  paid_at timestamptz,
  -- Bank or mobile money transaction reference.
  reference text,
  failure_message text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (period_end_on >= period_start_on),
  check (net_amount = gross_amount - commission_amount + adjustments_amount),
  check (status <> 'paid' or paid_at is not null)
);
select core.prepare_table('marketplace.payouts');
create index payouts_vendor_index on marketplace.payouts (vendor_id, period_end_on desc);

create table marketplace.payout_lines (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  payout_id uuid not null references marketplace.payouts (id) on delete cascade,
  order_id uuid references marketplace.orders (id),
  line_type text not null
    check (line_type in ('order', 'commission', 'short_delivery_credit', 'trade_credit_already_paid')),
  -- Positive adds to the payout, negative deducts.
  amount numeric(18,2) not null,
  description text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('marketplace.payout_lines');
create index payout_lines_payout_index on marketplace.payout_lines (payout_id);
create index payout_lines_order_index on marketplace.payout_lines (order_id) where order_id is not null;
