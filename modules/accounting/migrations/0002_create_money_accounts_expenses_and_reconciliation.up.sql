-- Module: accounting
-- Migration 0002: bank, mobile money and cash accounts, expenses, and statement reconciliation.

create table accounting.money_accounts (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid references core.branches (id),
  name text not null,
  account_type text not null check (account_type in ('bank', 'mobile_money_merchant', 'cash_safe', 'card_settlement')),
  -- CRDB, NMB, M-Pesa, Tigo Pesa, Airtel Money ...
  provider_name text,
  -- Only the last digits are stored, never the full account number.
  account_number_last_digits text check (account_number_last_digits ~ '^[0-9]{1,6}$'),
  ledger_account_id uuid references accounting.ledger_accounts (id),
  current_balance_amount numeric(18,2) not null default 0,
  is_active boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('accounting.money_accounts');

create table accounting.expense_categories (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  name text not null,
  name_translations jsonb not null default '{}'::jsonb,
  ledger_account_id uuid references accounting.ledger_accounts (id),
  -- Rent, electricity (LUKU), internet ... expected every period.
  is_repeating boolean not null default false,
  is_active boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('accounting.expense_categories');
create unique index expense_categories_name_unique_index on accounting.expense_categories (organization_id, lower(name)) where deleted_at is null;

create table accounting.expenses (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  expense_number text not null,
  branch_id uuid references core.branches (id),
  category_id uuid references accounting.expense_categories (id),
  description text not null,
  -- Total paid, including tax.
  amount numeric(18,2) not null check (amount >= 0),
  tax_amount numeric(18,2) not null default 0 check (tax_amount >= 0),
  -- Input VAT can be claimed back (needs a valid EFD receipt from a VAT-registered supplier).
  vat_claimable boolean not null default false,
  spent_on date not null,
  paid_from_money_account_id uuid references accounting.money_accounts (id),
  -- point_of_sale.shifts id when paid out of the till; no foreign key because point_of_sale is optional.
  paid_from_shift_reference uuid,
  supplier_name text,
  receipt_attachment_id uuid references core.attachments (id),
  receipt_status text not null default 'missing' check (receipt_status in ('attached', 'missing', 'read_by_assistant')),
  status text not null default 'draft' check (status in ('draft', 'waiting_for_approval', 'approved', 'rejected')),
  submitted_by_user_id uuid references core.users (id),
  approved_by_user_id uuid references core.users (id),
  approved_at timestamptz,
  repeats_every text not null default 'none' check (repeats_every in ('none', 'weekly', 'monthly')),
  -- accounting.journal_entries row created when the expense is approved.
  journal_entry_id uuid references accounting.journal_entries (id),
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, expense_number),
  check (tax_amount <= amount)
);
select core.prepare_table('accounting.expenses');
create index expenses_spent_index on accounting.expenses (organization_id, spent_on desc);
create index expenses_category_index on accounting.expenses (category_id);
create index expenses_waiting_index on accounting.expenses (organization_id) where status = 'waiting_for_approval' and deleted_at is null;

-- One uploaded bank or mobile money statement file.
create table accounting.statement_imports (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  money_account_id uuid not null references accounting.money_accounts (id),
  file_attachment_id uuid references core.attachments (id),
  period_start_on date not null,
  period_end_on date not null,
  opening_balance_amount numeric(18,2),
  closing_balance_amount numeric(18,2),
  line_count integer not null default 0,
  matched_count integer not null default 0,
  imported_by_user_id uuid references core.users (id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (period_end_on >= period_start_on)
);
select core.prepare_table('accounting.statement_imports');
create index statement_imports_account_index on accounting.statement_imports (money_account_id, period_start_on desc);

create table accounting.statement_lines (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  statement_import_id uuid not null references accounting.statement_imports (id) on delete cascade,
  transaction_at timestamptz not null,
  description text,
  -- Payer phone number for mobile money, account or name for bank transfers.
  counterparty_phone_or_account text,
  -- Positive for money in, negative for money out.
  amount numeric(18,2) not null,
  reference text,
  status text not null default 'unmatched' check (status in ('unmatched', 'matched', 'ignored', 'created_record')),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('accounting.statement_lines');
create index statement_lines_import_index on accounting.statement_lines (statement_import_id, transaction_at);
create index statement_lines_reference_index on accounting.statement_lines (organization_id, reference) where reference is not null;
create index statement_lines_unmatched_index on accounting.statement_lines (organization_id) where status = 'unmatched' and deleted_at is null;

-- Links a statement line to the record it pays for (sale payment, invoice payment, expense ...).
create table accounting.reconciliation_matches (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  statement_line_id uuid not null references accounting.statement_lines (id) on delete cascade,
  -- For example 'point_of_sale.sale_payments' or 'accounting.expenses'. No foreign key: the matched module may be optional.
  matched_table text not null,
  matched_row_id uuid not null,
  matched_amount numeric(18,2) not null,
  -- Statement amount minus record amount, usually the provider's fee.
  difference_amount numeric(18,2) not null default 0,
  match_type text not null check (match_type in ('exact', 'fee', 'manual')),
  matched_by_user_id uuid references core.users (id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('accounting.reconciliation_matches');
create index reconciliation_matches_line_index on accounting.reconciliation_matches (statement_line_id);
create index reconciliation_matches_target_index on accounting.reconciliation_matches (matched_table, matched_row_id);
