-- Module: accounting
-- Migration 0001: chart of accounts and double-entry journal, with a check that posted entries balance.

create schema accounting;

-- Chart of accounts.
create table accounting.ledger_accounts (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  code text not null,
  name text not null,
  name_translations jsonb not null default '{}'::jsonb,
  account_type text not null check (account_type in ('asset', 'liability', 'equity', 'income', 'expense')),
  parent_account_id uuid references accounting.ledger_accounts (id),
  -- Created by the system (sales, VAT payable, PAYE payable ...); cannot be deleted by the business.
  is_system_account boolean not null default false,
  normal_balance text not null check (normal_balance in ('debit', 'credit')),
  is_active boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('accounting.ledger_accounts');
create unique index ledger_accounts_code_unique_index on accounting.ledger_accounts (organization_id, code) where deleted_at is null;
create index ledger_accounts_parent_index on accounting.ledger_accounts (parent_account_id);

create table accounting.journal_entries (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  entry_number text not null,
  entry_on date not null,
  description text not null,
  -- The record this entry was made from, for example 'point_of_sale.sales' or 'hr.payroll_runs'. No foreign key: any module may post.
  source_table text,
  source_row_id uuid,
  status text not null default 'draft' check (status in ('draft', 'posted', 'reversed')),
  posted_by_user_id uuid references core.users (id),
  posted_at timestamptz,
  -- Set on the entry that cancels another one; the original is then marked reversed.
  reversal_of_entry_id uuid references accounting.journal_entries (id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, entry_number)
);
select core.prepare_table('accounting.journal_entries');
create index journal_entries_date_index on accounting.journal_entries (organization_id, entry_on desc);
create index journal_entries_source_index on accounting.journal_entries (source_table, source_row_id);

create table accounting.journal_lines (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  journal_entry_id uuid not null references accounting.journal_entries (id) on delete cascade,
  ledger_account_id uuid not null references accounting.ledger_accounts (id),
  branch_id uuid references core.branches (id),
  debit_amount numeric(18,2) not null default 0,
  credit_amount numeric(18,2) not null default 0,
  description text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  -- Each line is either a debit or a credit, never both and never zero.
  check (debit_amount >= 0 and credit_amount >= 0),
  check ((debit_amount > 0 and credit_amount = 0) or (credit_amount > 0 and debit_amount = 0))
);
select core.prepare_table('accounting.journal_lines');
create index journal_lines_entry_index on accounting.journal_lines (journal_entry_id);
create index journal_lines_account_index on accounting.journal_lines (ledger_account_id);

-- Rejects a posted entry whose debits and credits differ. Runs at commit (deferred),
-- so an entry and its lines can be written in any order inside one transaction.
create function accounting.check_journal_entry_balanced() returns trigger
language plpgsql as $$
declare
  entry_ids uuid[];
  checked_entry_id uuid;
  entry_status text;
  total_debit numeric(18,2);
  total_credit numeric(18,2);
begin
  if tg_table_name = 'journal_entries' then
    entry_ids := array[new.id];
  elsif tg_op = 'DELETE' then
    entry_ids := array[old.journal_entry_id];
  elsif tg_op = 'UPDATE' then
    entry_ids := array[new.journal_entry_id, old.journal_entry_id];
  else
    entry_ids := array[new.journal_entry_id];
  end if;

  foreach checked_entry_id in array entry_ids loop
    select status into entry_status
      from accounting.journal_entries
     where id = checked_entry_id and deleted_at is null;
    continue when entry_status is distinct from 'posted';

    select coalesce(sum(debit_amount), 0), coalesce(sum(credit_amount), 0)
      into total_debit, total_credit
      from accounting.journal_lines
     where journal_entry_id = checked_entry_id and deleted_at is null;

    if total_debit = 0 then
      raise exception 'Journal entry % cannot be posted without lines', checked_entry_id
        using errcode = 'check_violation';
    end if;
    if total_debit <> total_credit then
      raise exception 'Journal entry % is not balanced: debits % and credits %', checked_entry_id, total_debit, total_credit
        using errcode = 'check_violation';
    end if;
  end loop;
  return null;
end;
$$;

create constraint trigger journal_entry_balanced
  after insert or update on accounting.journal_entries
  deferrable initially deferred
  for each row execute function accounting.check_journal_entry_balanced();

create constraint trigger journal_lines_balanced
  after insert or update or delete on accounting.journal_lines
  deferrable initially deferred
  for each row execute function accounting.check_journal_entry_balanced();
