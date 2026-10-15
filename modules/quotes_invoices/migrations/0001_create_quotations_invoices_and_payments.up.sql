-- Module: quotes_invoices
-- Migration 0001: quotations, invoices and invoice payments for business customers.

create schema quotes_invoices;

create table quotes_invoices.quotations (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  quotation_number text not null,
  customer_id uuid not null references customers.customers (id),
  branch_id uuid references core.branches (id),
  status text not null default 'draft'
    check (status in ('draft', 'sent', 'viewed', 'accepted', 'declined', 'expired', 'converted')),
  valid_until_on date,
  sent_at timestamptz,
  -- How many times the customer opened the shared quotation link.
  view_count integer not null default 0 check (view_count >= 0),
  last_viewed_at timestamptz,
  subtotal_amount numeric(18,2) not null default 0,
  discount_amount numeric(18,2) not null default 0,
  tax_amount numeric(18,2) not null default 0,
  total_amount numeric(18,2) not null default 0,
  notes text,
  terms text,
  -- Set when the quotation becomes an invoice (foreign key added below, after invoices exists).
  converted_invoice_id uuid,
  prepared_by_user_id uuid references core.users (id),
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, quotation_number)
);
select core.prepare_table('quotes_invoices.quotations');
create index quotations_customer_index on quotes_invoices.quotations (customer_id);
create index quotations_status_index on quotes_invoices.quotations (organization_id, status);

create table quotes_invoices.quotation_lines (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  quotation_id uuid not null references quotes_invoices.quotations (id) on delete cascade,
  -- Null for free-text lines such as labour or transport.
  product_id uuid references catalog.products (id),
  product_variant_id uuid references catalog.product_variants (id),
  description text not null,
  quantity numeric(18,3) not null check (quantity > 0),
  -- Price from the price list before any negotiated change.
  list_unit_price numeric(18,2),
  unit_price numeric(18,2) not null check (unit_price >= 0),
  discount_percent numeric(6,3) not null default 0 check (discount_percent between 0 and 100),
  tax_rate_percent numeric(6,3) not null default 0,
  line_total_amount numeric(18,2) not null default 0,
  -- Cost at quoting time, so margin can be shown to managers.
  cost_price_snapshot numeric(18,2),
  sort_order integer not null default 0,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('quotes_invoices.quotation_lines');
create index quotation_lines_quotation_index on quotes_invoices.quotation_lines (quotation_id, sort_order);
create index quotation_lines_product_index on quotes_invoices.quotation_lines (product_id);

create table quotes_invoices.invoices (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  invoice_number text not null,
  customer_id uuid not null references customers.customers (id),
  branch_id uuid references core.branches (id),
  quotation_id uuid references quotes_invoices.quotations (id),
  issued_on date,
  due_on date,
  status text not null default 'draft'
    check (status in ('draft', 'issued', 'partly_paid', 'paid', 'overdue', 'cancelled', 'credited')),
  subtotal_amount numeric(18,2) not null default 0,
  discount_amount numeric(18,2) not null default 0,
  tax_amount numeric(18,2) not null default 0,
  total_amount numeric(18,2) not null default 0,
  paid_amount numeric(18,2) not null default 0,
  balance_due_amount numeric(18,2) generated always as (total_amount - paid_amount) stored,
  -- TRA electronic fiscal device receipt number and verification code.
  fiscal_receipt_number text,
  fiscal_verification_code text,
  delivered_on date,
  -- Mobile money or card payment link sent with the invoice.
  payment_link text,
  notes text,
  terms text,
  issued_by_user_id uuid references core.users (id),
  -- accounting.journal_entries id once posted; no foreign key because accounting is optional.
  journal_entry_id uuid,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, invoice_number),
  check (due_on is null or issued_on is null or due_on >= issued_on)
);
select core.prepare_table('quotes_invoices.invoices');
create index invoices_customer_index on quotes_invoices.invoices (customer_id);
create index invoices_open_due_index on quotes_invoices.invoices (organization_id, due_on)
  where status in ('issued', 'partly_paid', 'overdue') and deleted_at is null;

alter table quotes_invoices.quotations
  add constraint quotations_converted_invoice_foreign_key
  foreign key (converted_invoice_id) references quotes_invoices.invoices (id);

create table quotes_invoices.invoice_lines (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  invoice_id uuid not null references quotes_invoices.invoices (id) on delete cascade,
  quotation_line_id uuid references quotes_invoices.quotation_lines (id),
  product_id uuid references catalog.products (id),
  product_variant_id uuid references catalog.product_variants (id),
  description text not null,
  quantity numeric(18,3) not null check (quantity > 0),
  list_unit_price numeric(18,2),
  unit_price numeric(18,2) not null check (unit_price >= 0),
  discount_percent numeric(6,3) not null default 0 check (discount_percent between 0 and 100),
  tax_rate_percent numeric(6,3) not null default 0,
  line_total_amount numeric(18,2) not null default 0,
  cost_price_snapshot numeric(18,2),
  sort_order integer not null default 0,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('quotes_invoices.invoice_lines');
create index invoice_lines_invoice_index on quotes_invoices.invoice_lines (invoice_id, sort_order);
create index invoice_lines_product_index on quotes_invoices.invoice_lines (product_id);

create table quotes_invoices.invoice_payments (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  invoice_id uuid not null references quotes_invoices.invoices (id) on delete cascade,
  amount numeric(18,2) not null check (amount > 0),
  method text not null check (method in ('cash', 'card', 'mobile_money', 'bank_transfer', 'cheque', 'customer_credit')),
  -- Mobile money transaction code, bank reference or cheque number.
  reference text,
  paid_at timestamptz not null default now(),
  recorded_by_user_id uuid references core.users (id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('quotes_invoices.invoice_payments');
create index invoice_payments_invoice_index on quotes_invoices.invoice_payments (invoice_id);
create index invoice_payments_reference_index on quotes_invoices.invoice_payments (organization_id, reference) where reference is not null;
