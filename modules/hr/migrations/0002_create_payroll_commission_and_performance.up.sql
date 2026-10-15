-- Module: hr
-- Migration 0002: payroll runs, payslips, salary advances, sales commission and performance reviews.
-- Tanzanian statutory terms are spelled out: income tax (PAYE), pension contribution (NSSF),
-- skills development levy (SDL) and workers compensation (WCF).

-- One pay period for the business (or one branch). Moves draft → calculated → waiting_for_approval → approved → paid.
create table hr.payroll_runs (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  run_number text not null,
  -- Null means every branch.
  branch_id uuid references core.branches (id),
  period_start_on date not null,
  period_end_on date not null,
  pay_on date not null,
  status text not null default 'draft'
    check (status in ('draft', 'calculated', 'waiting_for_approval', 'approved', 'paid', 'cancelled')),
  prepared_by_user_id uuid references core.users (id),
  approved_by_user_id uuid references core.users (id),
  approved_at timestamptz,
  paid_at timestamptz,
  gross_amount numeric(18,2) not null default 0,
  employee_deductions_amount numeric(18,2) not null default 0,
  employer_contributions_amount numeric(18,2) not null default 0,
  net_amount numeric(18,2) not null default 0,
  -- accounting.journal_entries id once posted; no foreign key because accounting is optional.
  journal_entry_id uuid,
  notes text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, run_number),
  check (period_end_on >= period_start_on)
);
select core.prepare_table('hr.payroll_runs');
create index payroll_runs_period_index on hr.payroll_runs (organization_id, period_start_on desc);

create table hr.payslips (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  payroll_run_id uuid not null references hr.payroll_runs (id) on delete cascade,
  employee_id uuid not null references hr.employees (id),
  basic_amount numeric(18,2) not null default 0,
  overtime_amount numeric(18,2) not null default 0,
  commission_amount numeric(18,2) not null default 0,
  allowances_amount numeric(18,2) not null default 0,
  gross_amount numeric(18,2) not null default 0,
  -- Employee share of NSSF, deducted before income tax.
  pension_contribution_amount numeric(18,2) not null default 0,
  taxable_amount numeric(18,2) not null default 0,
  -- PAYE.
  income_tax_amount numeric(18,2) not null default 0,
  advance_deduction_amount numeric(18,2) not null default 0,
  other_deductions_amount numeric(18,2) not null default 0,
  net_amount numeric(18,2) not null default 0,
  -- Employer costs on top of gross pay; not deducted from the employee.
  employer_pension_contribution_amount numeric(18,2) not null default 0,
  skills_development_levy_amount numeric(18,2) not null default 0,
  workers_compensation_amount numeric(18,2) not null default 0,
  days_worked numeric(6,2),
  paid_to_method text check (paid_to_method in ('bank', 'mobile_money', 'cash')),
  paid_at timestamptz,
  -- Bank or mobile money transaction reference of the salary payment.
  payment_reference text,
  sent_to_employee_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (payroll_run_id, employee_id)
);
select core.prepare_table('hr.payslips');
create index payslips_employee_index on hr.payslips (employee_id);

-- Itemised breakdown printed on the payslip.
create table hr.payslip_lines (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  payslip_id uuid not null references hr.payslips (id) on delete cascade,
  line_type text not null check (line_type in ('earning', 'deduction', 'employer_contribution')),
  -- Stable machine key such as 'basic', 'overtime', 'income_tax', 'pension_contribution', 'transport_allowance'.
  code text not null,
  description text not null,
  amount numeric(18,2) not null,
  sort_order integer not null default 0,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('hr.payslip_lines');
create index payslip_lines_payslip_index on hr.payslip_lines (payslip_id, sort_order);

create table hr.salary_advances (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  employee_id uuid not null references hr.employees (id) on delete cascade,
  amount numeric(18,2) not null check (amount > 0),
  repaid_amount numeric(18,2) not null default 0 check (repaid_amount >= 0),
  reason text,
  given_on date,
  -- The payroll run whose payslip deducts this advance.
  repay_from_run_id uuid references hr.payroll_runs (id),
  status text not null default 'requested'
    check (status in ('requested', 'approved', 'declined', 'given', 'repaid', 'cancelled')),
  approved_by_user_id uuid references core.users (id),
  given_by_user_id uuid references core.users (id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('hr.salary_advances');
create index salary_advances_employee_index on hr.salary_advances (employee_id);
create index salary_advances_outstanding_index on hr.salary_advances (organization_id) where status = 'given' and deleted_at is null;

create table hr.commission_rules (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  name text not null,
  -- core.roles key the rule applies to, for example 'cashier'. Null means everyone.
  applies_to_role_key text,
  branch_id uuid references core.branches (id),
  basis text not null
    check (basis in ('net_sales_over_target', 'percent_of_sales', 'fixed_per_item', 'per_new_loyalty_member', 'percent_of_paid_invoices')),
  rate_percent numeric(6,3),
  fixed_amount numeric(18,2),
  monthly_target_amount numeric(18,2),
  -- What does not count, for example {"category_ids": [...], "product_ids": [...], "discounted_sales": true}.
  excludes jsonb not null default '{}'::jsonb,
  is_active boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (rate_percent is not null or fixed_amount is not null)
);
select core.prepare_table('hr.commission_rules');

create table hr.commission_earnings (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  employee_id uuid not null references hr.employees (id) on delete cascade,
  commission_rule_id uuid not null references hr.commission_rules (id),
  period_start_on date not null,
  period_end_on date not null,
  -- The sales, items or invoices the commission was calculated on.
  basis_amount numeric(18,2) not null default 0,
  earned_amount numeric(18,2) not null default 0,
  -- Set once the earning is paid through a payslip.
  payslip_id uuid references hr.payslips (id),
  -- Record that earned it, for example 'point_of_sale.sales' or 'quotes_invoices.invoices'. No foreign key: those modules are optional.
  source_table text,
  source_row_id uuid,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (period_end_on >= period_start_on)
);
select core.prepare_table('hr.commission_earnings');
create index commission_earnings_employee_period_index on hr.commission_earnings (employee_id, period_start_on);
create index commission_earnings_unpaid_index on hr.commission_earnings (organization_id) where payslip_id is null and deleted_at is null;
create index commission_earnings_source_index on hr.commission_earnings (source_table, source_row_id);

create table hr.performance_reviews (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  employee_id uuid not null references hr.employees (id) on delete cascade,
  reviewer_user_id uuid references core.users (id),
  review_type text not null default 'yearly'
    check (review_type in ('end_of_probation', 'quarterly', 'half_yearly', 'yearly', 'one_off')),
  due_on date,
  completed_on date,
  -- {"punctuality": 4, "customer_service": 5, "cash_handling": 3}, out of 5.
  scores jsonb not null default '{}'::jsonb,
  notes text,
  status text not null default 'scheduled' check (status in ('scheduled', 'in_progress', 'completed', 'cancelled')),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('hr.performance_reviews');
create index performance_reviews_employee_index on hr.performance_reviews (employee_id, due_on desc);
