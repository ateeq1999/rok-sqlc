-- Module: point_of_sale
-- Migration 0002: manager approvals, drawer cash movements and fiscal (tax authority) receipts.

-- A manager's sign-off for a risky register action, checked against core.approval_rules.
create table point_of_sale.sale_approvals (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  sale_id uuid references point_of_sale.sales (id) on delete cascade,
  sale_line_id uuid references point_of_sale.sale_lines (id) on delete cascade,
  approval_rule_id uuid references core.approval_rules (id),
  action text not null check (action in ('void', 'refund', 'discount', 'price_override', 'credit_sale')),
  status text not null default 'pending' check (status in ('pending', 'approved', 'rejected')),
  requested_by_user_id uuid not null references core.users (id),
  approved_by_user_id uuid references core.users (id),
  approval_method text check (approval_method in ('personal_identification_number', 'in_app', 'text_message')),
  reason text,
  -- Amount or percent asked for, for example a 15% discount or a 50,000 refund.
  requested_amount numeric(18,2),
  requested_percent numeric(6,3),
  -- The approver's role limits in force at the time, for example {"maximum_discount_percent": 20}.
  limits_used jsonb not null default '{}'::jsonb,
  device_id uuid references core.devices (id),
  decided_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('point_of_sale.sale_approvals');
create index sale_approvals_sale_index on point_of_sale.sale_approvals (sale_id);
create index sale_approvals_approver_index on point_of_sale.sale_approvals (approved_by_user_id, created_at desc);

-- Cash put into or taken out of the drawer that is not a sale.
create table point_of_sale.cash_movements (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  shift_id uuid not null references point_of_sale.shifts (id),
  movement_type text not null check (movement_type in ('cash_in', 'paid_out', 'drop_to_safe')),
  amount numeric(18,2) not null check (amount > 0),
  reason text not null,
  recorded_by_user_id uuid not null references core.users (id),
  approved_by_user_id uuid references core.users (id),
  occurred_at timestamptz not null default now(),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('point_of_sale.cash_movements');
create index cash_movements_shift_index on point_of_sale.cash_movements (shift_id);

-- Receipt signed by the tax authority's fiscal device or online service (Tanzania Revenue Authority virtual or electronic fiscal device).
create table point_of_sale.fiscal_receipts (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  sale_id uuid not null references point_of_sale.sales (id),
  fiscal_device_id uuid references core.devices (id),
  receipt_number text,
  verification_code text,
  verification_url text,
  status text not null default 'pending' check (status in ('pending', 'signed', 'failed')),
  signed_at timestamptz,
  attempt_count integer not null default 0,
  last_error_message text,
  -- Full reply from the fiscal device or tax authority service.
  response jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('point_of_sale.fiscal_receipts');
create unique index fiscal_receipts_sale_unique_index on point_of_sale.fiscal_receipts (sale_id) where deleted_at is null;
create unique index fiscal_receipts_number_unique_index on point_of_sale.fiscal_receipts (organization_id, fiscal_device_id, receipt_number) where receipt_number is not null;
create index fiscal_receipts_waiting_index on point_of_sale.fiscal_receipts (organization_id, created_at) where status in ('pending', 'failed');
