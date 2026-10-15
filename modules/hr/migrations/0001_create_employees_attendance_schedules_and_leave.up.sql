-- Module: hr
-- Migration 0001: employees, contracts, documents, attendance, shift schedules and leave.

create schema hr;

create table hr.employees (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  -- Null for staff who never sign in to the system (cleaners, guards, casual labour).
  user_id uuid references core.users (id),
  branch_id uuid references core.branches (id),
  employee_number text,
  full_name text not null,
  job_title text,
  employment_type text not null default 'permanent'
    check (employment_type in ('permanent', 'fixed_term', 'part_time', 'casual')),
  started_on date not null,
  ended_on date,
  status text not null default 'active' check (status in ('active', 'on_leave', 'suspended', 'left')),
  basic_monthly_pay_amount numeric(18,2) not null default 0 check (basic_monthly_pay_amount >= 0),
  pay_frequency text not null default 'monthly'
    check (pay_frequency in ('monthly', 'every_two_weeks', 'weekly', 'daily')),
  paid_to_method text not null default 'mobile_money' check (paid_to_method in ('bank', 'mobile_money', 'cash')),
  -- Bank account or mobile money number the salary is sent to.
  paid_to_account_reference text,
  -- NSSF membership number.
  pension_member_number text,
  -- TRA TIN, needed for PAYE returns.
  tax_identification_number text,
  phone_number text,
  emergency_contact_name text,
  emergency_contact_phone_number text,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (ended_on is null or ended_on >= started_on)
);
select core.prepare_table('hr.employees');
create unique index employees_employee_number_unique_index on hr.employees (organization_id, employee_number) where employee_number is not null and deleted_at is null;
create unique index employees_user_unique_index on hr.employees (user_id) where user_id is not null and deleted_at is null;
create index employees_branch_index on hr.employees (branch_id);

create table hr.employment_contracts (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  employee_id uuid not null references hr.employees (id) on delete cascade,
  employment_type text not null check (employment_type in ('permanent', 'fixed_term', 'part_time', 'casual')),
  job_title text,
  starts_on date not null,
  -- Null for open-ended (permanent) contracts.
  ends_on date,
  probation_ends_on date,
  basic_monthly_pay_amount numeric(18,2) not null default 0 check (basic_monthly_pay_amount >= 0),
  signed_on date,
  attachment_id uuid references core.attachments (id),
  notes text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (ends_on is null or ends_on >= starts_on)
);
select core.prepare_table('hr.employment_contracts');
create index employment_contracts_employee_index on hr.employment_contracts (employee_id, starts_on desc);

create table hr.employee_documents (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  employee_id uuid not null references hr.employees (id) on delete cascade,
  document_type text not null
    check (document_type in ('national_identity_card', 'passport', 'work_permit', 'tax_registration', 'pension_registration', 'academic_certificate', 'medical_certificate', 'driving_licence', 'reference_letter', 'other')),
  title text,
  attachment_id uuid not null references core.attachments (id),
  -- Reminders are raised before this date (work permits, driving licences).
  expires_on date,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('hr.employee_documents');
create index employee_documents_employee_index on hr.employee_documents (employee_id);
create index employee_documents_expiry_index on hr.employee_documents (organization_id, expires_on) where expires_on is not null and deleted_at is null;

create table hr.attendance_records (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  employee_id uuid not null references hr.employees (id) on delete cascade,
  branch_id uuid not null references core.branches (id),
  -- The till or phone used to clock in.
  device_id uuid references core.devices (id),
  clock_in_at timestamptz not null,
  clock_out_at timestamptz,
  clock_in_method text not null default 'personal_identification_number_with_photo'
    check (clock_in_method in ('personal_identification_number_with_photo', 'rider_app_location', 'manager_entry')),
  -- Photo taken at clock-in to stop buddy punching.
  photo_attachment_id uuid references core.attachments (id),
  minutes_late integer not null default 0 check (minutes_late >= 0),
  break_minutes integer not null default 0 check (break_minutes >= 0),
  worked_hours numeric(6,2),
  overtime_hours numeric(6,2) not null default 0 check (overtime_hours >= 0),
  overtime_status text check (overtime_status in ('pending', 'approved', 'rejected')),
  late_reason text,
  approved_by_user_id uuid references core.users (id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (clock_out_at is null or clock_out_at >= clock_in_at)
);
select core.prepare_table('hr.attendance_records');
create index attendance_records_employee_index on hr.attendance_records (employee_id, clock_in_at desc);
create index attendance_records_branch_day_index on hr.attendance_records (branch_id, clock_in_at);
-- Staff still clocked in, for the "who is here now" view.
create index attendance_records_open_index on hr.attendance_records (organization_id) where clock_out_at is null and deleted_at is null;

create table hr.shift_templates (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid references core.branches (id),
  name text not null,
  starts_at time not null,
  -- May be earlier than starts_at for shifts that run past midnight.
  ends_at time not null,
  color text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('hr.shift_templates');

create table hr.scheduled_shifts (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  employee_id uuid not null references hr.employees (id) on delete cascade,
  branch_id uuid not null references core.branches (id),
  work_on date not null,
  starts_at time not null,
  ends_at time not null,
  shift_template_id uuid references hr.shift_templates (id),
  -- Cashier, packer, rider ... free text so each business can use its own words.
  role_on_shift text,
  status text not null default 'draft' check (status in ('draft', 'published', 'swapped', 'cancelled')),
  -- Set when the shift was swapped from another employee's shift.
  swapped_from_shift_id uuid references hr.scheduled_shifts (id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('hr.scheduled_shifts');
create unique index scheduled_shifts_employee_start_unique_index on hr.scheduled_shifts (employee_id, work_on, starts_at)
  where deleted_at is null and status <> 'cancelled';
create index scheduled_shifts_branch_day_index on hr.scheduled_shifts (branch_id, work_on);

create table hr.leave_types (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  name text not null,
  days_per_year numeric(6,2) not null default 0 check (days_per_year >= 0),
  is_paid boolean not null default true,
  carries_over_maximum_days numeric(6,2) not null default 0 check (carries_over_maximum_days >= 0),
  -- A sick note is required when one request is longer than this many days. Null means never.
  needs_document_after_days numeric(6,2),
  is_active boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('hr.leave_types');
create unique index leave_types_name_unique_index on hr.leave_types (organization_id, lower(name)) where deleted_at is null;

create table hr.leave_balances (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  employee_id uuid not null references hr.employees (id) on delete cascade,
  leave_type_id uuid not null references hr.leave_types (id),
  year integer not null check (year between 2000 and 2200),
  entitled_days numeric(6,2) not null default 0,
  taken_days numeric(6,2) not null default 0,
  carried_over_days numeric(6,2) not null default 0,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (employee_id, leave_type_id, year)
);
select core.prepare_table('hr.leave_balances');

create table hr.leave_requests (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  employee_id uuid not null references hr.employees (id) on delete cascade,
  leave_type_id uuid not null references hr.leave_types (id),
  starts_on date not null,
  ends_on date not null,
  -- Working days taken, which may be less than the calendar range (weekends, public holidays, half days).
  day_count numeric(6,2) not null check (day_count > 0),
  reason text,
  supporting_attachment_id uuid references core.attachments (id),
  status text not null default 'pending' check (status in ('pending', 'approved', 'declined', 'cancelled')),
  decided_by_user_id uuid references core.users (id),
  decided_at timestamptz,
  decision_note text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (ends_on >= starts_on)
);
select core.prepare_table('hr.leave_requests');
create index leave_requests_employee_index on hr.leave_requests (employee_id, starts_on desc);
create index leave_requests_pending_index on hr.leave_requests (organization_id) where status = 'pending' and deleted_at is null;
