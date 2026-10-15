-- Module: salon (hair, beauty, barber, spa)
-- Migration 0001: bookable services, staff skills, appointments and prepaid service packages.

create schema salon;

-- Booking details for a service product; the price comes from catalog.products.
create table salon.bookable_services (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  product_id uuid not null references catalog.products (id) on delete cascade,
  duration_minutes integer not null check (duration_minutes > 0),
  -- Cleaning or drying time blocked after the service.
  buffer_minutes integer not null default 0 check (buffer_minutes >= 0),
  -- Default share of the service price the stylist earns.
  commission_percent numeric(6,3) not null default 0,
  can_be_booked_online boolean not null default true,
  is_active boolean not null default true,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('salon.bookable_services');
create unique index bookable_services_product_unique_index on salon.bookable_services (product_id) where deleted_at is null;

-- Which staff can perform which services.
create table salon.staff_service_skills (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  user_id uuid not null references core.users (id) on delete cascade,
  bookable_service_id uuid not null references salon.bookable_services (id) on delete cascade,
  -- Overrides the service commission for this person, if set.
  commission_percent numeric(6,3),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('salon.staff_service_skills');
create unique index staff_service_skills_unique_index on salon.staff_service_skills (user_id, bookable_service_id) where deleted_at is null;
create index staff_service_skills_bookable_service_index on salon.staff_service_skills (bookable_service_id);

-- "5 blow-dries for 100,000": prepaid sessions of one service.
create table salon.service_packages (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  name text not null,
  bookable_service_id uuid not null references salon.bookable_services (id),
  sessions_included integer not null check (sessions_included > 0),
  price_amount numeric(18,2) not null check (price_amount >= 0),
  -- Days the sessions stay usable after purchase.
  valid_days integer check (valid_days is null or valid_days > 0),
  is_active boolean not null default true,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('salon.service_packages');

-- A package a customer bought and how many sessions remain.
create table salon.customer_package_balances (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  customer_id uuid not null references customers.customers (id) on delete cascade,
  service_package_id uuid not null references salon.service_packages (id),
  sessions_left integer not null check (sessions_left >= 0),
  expires_on date,
  bought_sale_id uuid references point_of_sale.sales (id) on delete set null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('salon.customer_package_balances');
create index customer_package_balances_customer_index on salon.customer_package_balances (customer_id);

create table salon.appointments (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid not null references core.branches (id),
  customer_id uuid references customers.customers (id) on delete set null,
  -- For walk-ins who are not saved customers.
  guest_name text,
  guest_phone_number text,
  staff_user_id uuid references core.users (id) on delete set null,
  starts_at timestamptz not null,
  ends_at timestamptz not null,
  status text not null default 'booked'
    check (status in ('booked', 'arrived', 'in_service', 'completed', 'no_show', 'cancelled')),
  source text not null default 'walk_in' check (source in ('walk_in', 'phone', 'online')),
  deposit_amount numeric(18,2) not null default 0,
  sale_id uuid references point_of_sale.sales (id) on delete set null,
  booked_by_user_id uuid references core.users (id),
  reminder_sent_at timestamptz,
  notes text,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (ends_at > starts_at)
);
select core.prepare_table('salon.appointments');
create index appointments_staff_time_index on salon.appointments (staff_user_id, starts_at);
create index appointments_branch_time_index on salon.appointments (branch_id, starts_at);
create index appointments_customer_index on salon.appointments (customer_id);

-- Each service within an appointment (braids + nails), possibly by different staff.
create table salon.appointment_services (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  appointment_id uuid not null references salon.appointments (id) on delete cascade,
  bookable_service_id uuid not null references salon.bookable_services (id),
  staff_user_id uuid references core.users (id) on delete set null,
  starts_at timestamptz,
  duration_minutes integer not null check (duration_minutes > 0),
  price_amount numeric(18,2) not null default 0,
  -- Set when the service is paid from a prepaid package session instead of cash.
  customer_package_balance_id uuid references salon.customer_package_balances (id) on delete set null,
  sale_line_id uuid references point_of_sale.sale_lines (id) on delete set null,
  sort_order integer not null default 0,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('salon.appointment_services');
create index appointment_services_appointment_index on salon.appointment_services (appointment_id);
