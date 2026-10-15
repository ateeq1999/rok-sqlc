-- Module: point_of_sale
-- Migration 0001: register shifts, sales, sale lines and sale payments.

create schema point_of_sale;

-- One cashier session on one register, from opening float to end-of-day cash count.
create table point_of_sale.shifts (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid not null references core.branches (id),
  device_id uuid not null references core.devices (id),
  status text not null default 'open' check (status in ('open', 'closed')),
  opened_by_user_id uuid not null references core.users (id),
  opened_at timestamptz not null default now(),
  closed_by_user_id uuid references core.users (id),
  closed_at timestamptz,
  -- Cash placed in the drawer at the start of the shift.
  opening_float_amount numeric(18,2) not null default 0 check (opening_float_amount >= 0),
  -- Opening float plus cash sales and cash in, minus cash refunds, paid outs and drops to safe.
  expected_cash_amount numeric(18,2),
  counted_cash_amount numeric(18,2),
  -- Counted minus expected: negative is a shortage, positive an overage.
  cash_difference_amount numeric(18,2),
  -- End-of-day ("Z") report totals frozen at closing: sales by method, taxes, refunds, voids.
  end_of_day_report jsonb,
  closing_notes text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (status = 'open' or closed_at is not null)
);
select core.prepare_table('point_of_sale.shifts');
-- Only one open shift per register at a time.
create unique index shifts_open_per_device_unique_index on point_of_sale.shifts (device_id) where status = 'open' and deleted_at is null;
create index shifts_branch_index on point_of_sale.shifts (branch_id, opened_at desc);

create table point_of_sale.sales (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  sale_number text not null,
  sale_type text not null default 'sale' check (sale_type in ('sale', 'return', 'exchange')),
  -- For returns and exchanges, the sale the goods were originally bought on.
  original_sale_id uuid references point_of_sale.sales (id),
  status text not null default 'open_tab'
    check (status in ('held', 'open_tab', 'completed', 'voided', 'refunded')),
  channel text not null default 'register' check (channel in ('register', 'online', 'phone', 'waiter_app')),
  branch_id uuid not null references core.branches (id),
  shift_id uuid references point_of_sale.shifts (id),
  device_id uuid references core.devices (id),
  cashier_user_id uuid references core.users (id),
  customer_id uuid references customers.customers (id),
  price_list_id uuid references catalog.price_lists (id),
  subtotal_amount numeric(18,2) not null default 0,
  discount_amount numeric(18,2) not null default 0 check (discount_amount >= 0),
  tax_amount numeric(18,2) not null default 0,
  total_amount numeric(18,2) not null default 0,
  paid_amount numeric(18,2) not null default 0,
  change_given_amount numeric(18,2) not null default 0 check (change_given_amount >= 0),
  -- Name the cashier gives a parked sale, for example "Mama Neema, blue bag".
  held_label text,
  notes text,
  completed_at timestamptz,
  voided_at timestamptz,
  -- When the register created the sale while offline; null if created online.
  offline_created_at timestamptz,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, sale_number),
  check (original_sale_id is null or sale_type in ('return', 'exchange'))
);
select core.prepare_table('point_of_sale.sales');
create index sales_completed_index on point_of_sale.sales (organization_id, branch_id, completed_at desc);
create index sales_shift_index on point_of_sale.sales (shift_id);
create index sales_customer_index on point_of_sale.sales (customer_id) where customer_id is not null;
create index sales_original_sale_index on point_of_sale.sales (original_sale_id) where original_sale_id is not null;
create index sales_waiting_index on point_of_sale.sales (organization_id, branch_id, status) where status in ('held', 'open_tab');

create table point_of_sale.sale_lines (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  sale_id uuid not null references point_of_sale.sales (id) on delete cascade,
  line_number integer not null,
  product_id uuid references catalog.products (id),
  product_variant_id uuid references catalog.product_variants (id),
  -- Name printed on the receipt, copied at time of sale so later product renames do not change history.
  description text not null,
  -- Negative on return lines.
  quantity numeric(18,3) not null check (quantity <> 0),
  unit_price numeric(18,2) not null,
  -- Catalogue price before any manual price override, to report on overrides.
  original_unit_price numeric(18,2),
  -- Cost at time of sale, for margin reporting.
  unit_cost numeric(18,2),
  discount_amount numeric(18,2) not null default 0 check (discount_amount >= 0),
  tax_rate_id uuid references catalog.tax_rates (id),
  tax_rate_percent numeric(6,3) not null default 0,
  tax_amount numeric(18,2) not null default 0,
  line_total_amount numeric(18,2) not null,
  stock_location_id uuid references inventory.stock_locations (id),
  stock_batch_id uuid references inventory.stock_batches (id),
  stock_serial_number_id uuid references inventory.stock_serial_numbers (id),
  -- On a return, the line of the original sale being returned.
  original_sale_line_id uuid references point_of_sale.sale_lines (id),
  return_reason text,
  -- Whether a returned item goes back into sellable stock.
  is_returned_to_stock boolean,
  -- Staff member credited with the sale, for commission.
  sold_by_user_id uuid references core.users (id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (sale_id, line_number)
);
select core.prepare_table('point_of_sale.sale_lines');
create index sale_lines_product_index on point_of_sale.sale_lines (product_id, product_variant_id);
create index sale_lines_sold_by_index on point_of_sale.sale_lines (sold_by_user_id) where sold_by_user_id is not null;

create table point_of_sale.sale_payments (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  sale_id uuid not null references point_of_sale.sales (id) on delete cascade,
  shift_id uuid references point_of_sale.shifts (id),
  payment_method text not null
    check (payment_method in ('cash', 'card', 'mobile_money', 'credit_account', 'gift_card', 'loyalty_points', 'bank_transfer')),
  -- Negative when money is paid back to the customer on a return.
  amount numeric(18,2) not null check (amount <> 0),
  -- Cash handed over by the customer, before change.
  tendered_amount numeric(18,2),
  -- Card approval code, mobile money transaction id, bank reference or gift card number.
  reference text,
  mobile_money_provider text
    check (mobile_money_provider in ('m_pesa', 'airtel_money', 'mixx_by_yas', 'halopesa', 'azampesa', 'other')),
  payer_phone_number text,
  credit_account_id uuid references customers.credit_accounts (id),
  status text not null default 'succeeded' check (status in ('pending', 'succeeded', 'failed', 'refunded')),
  -- Raw reply from the card terminal or mobile money gateway.
  provider_response jsonb,
  received_by_user_id uuid references core.users (id),
  received_at timestamptz not null default now(),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (payment_method <> 'mobile_money' or mobile_money_provider is not null),
  check (payment_method <> 'credit_account' or credit_account_id is not null)
);
select core.prepare_table('point_of_sale.sale_payments');
create index sale_payments_sale_index on point_of_sale.sale_payments (sale_id);
create index sale_payments_shift_method_index on point_of_sale.sale_payments (shift_id, payment_method);
create index sale_payments_reference_index on point_of_sale.sale_payments (organization_id, reference) where reference is not null;
create index sale_payments_pending_index on point_of_sale.sale_payments (organization_id, created_at) where status = 'pending';
