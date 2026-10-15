-- Module: delivery
-- Migration 0001: riders, delivery jobs and their tracking, and rider cash handovers.

create schema delivery;

-- Staff riders link to a user; partner riders (boda boda, courier company) have no login.
create table delivery.riders (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  rider_type text not null default 'staff' check (rider_type in ('staff', 'partner')),
  user_id uuid references core.users (id),
  full_name text not null,
  phone_number text,
  partner_company_name text,
  vehicle_type text not null default 'motorcycle'
    check (vehicle_type in ('motorcycle', 'bicycle', 'tricycle', 'car', 'van', 'truck', 'on_foot')),
  vehicle_registration_number text,
  pay_per_trip_amount numeric(18,2) not null default 0,
  home_branch_id uuid references core.branches (id),
  is_active boolean not null default true,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (rider_type <> 'staff' or user_id is not null),
  check (rider_type <> 'partner' or phone_number is not null)
);
select core.prepare_table('delivery.riders');
create unique index riders_user_unique_index on delivery.riders (user_id) where user_id is not null and deleted_at is null;

-- Cash a rider brings back from cash-on-delivery jobs, counted by a cashier.
create table delivery.rider_cash_handovers (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  rider_id uuid not null references delivery.riders (id),
  branch_id uuid references core.branches (id),
  -- Till shift the cash went into (point_of_sale.shifts id); no foreign key because point_of_sale is optional.
  shift_reference uuid,
  expected_amount numeric(18,2),
  amount numeric(18,2) not null check (amount >= 0),
  received_by_user_id uuid not null references core.users (id),
  counted_at timestamptz not null default now(),
  note text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('delivery.rider_cash_handovers');
create index rider_cash_handovers_rider_index on delivery.rider_cash_handovers (rider_id, counted_at desc);

create table delivery.delivery_jobs (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  job_number text not null,
  -- The document being delivered: 'point_of_sale.sales', 'online_shop.online_orders', 'quotes_invoices.invoices' + its id.
  source_table text,
  source_row_id uuid,
  customer_id uuid references customers.customers (id),
  branch_id uuid not null references core.branches (id),
  rider_id uuid references delivery.riders (id),
  recipient_name text,
  recipient_phone_number text,
  address_text text not null,
  latitude numeric(9,6),
  longitude numeric(9,6),
  delivery_instructions text,
  cash_to_collect_amount numeric(18,2) not null default 0 check (cash_to_collect_amount >= 0),
  cash_collected_amount numeric(18,2),
  delivery_fee_amount numeric(18,2) not null default 0,
  status text not null default 'ready'
    check (status in ('ready', 'assigned', 'picked_up', 'delivered', 'failed', 'returned', 'cancelled')),
  promised_by_at timestamptz,
  assigned_at timestamptz,
  picked_up_at timestamptz,
  delivered_at timestamptz,
  proof_attachment_id uuid references core.attachments (id),
  failure_reason text,
  -- Jobs a rider carries together share a trip_reference; sequence_in_trip is the drop order.
  trip_reference uuid,
  sequence_in_trip integer,
  rider_cash_handover_id uuid references delivery.rider_cash_handovers (id),
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, job_number),
  check ((source_table is null) = (source_row_id is null)),
  check (status not in ('assigned', 'picked_up', 'delivered') or rider_id is not null),
  check (status <> 'failed' or failure_reason is not null)
);
select core.prepare_table('delivery.delivery_jobs');
create index delivery_jobs_open_index on delivery.delivery_jobs (organization_id, branch_id, status)
  where status in ('ready', 'assigned', 'picked_up');
create index delivery_jobs_rider_index on delivery.delivery_jobs (rider_id, status);
create index delivery_jobs_source_index on delivery.delivery_jobs (source_table, source_row_id);
create index delivery_jobs_customer_index on delivery.delivery_jobs (customer_id);
create index delivery_jobs_trip_index on delivery.delivery_jobs (trip_reference, sequence_in_trip) where trip_reference is not null;
-- Delivered cash still with the rider.
create index delivery_jobs_cash_pending_index on delivery.delivery_jobs (rider_id)
  where status = 'delivered' and cash_to_collect_amount > 0 and rider_cash_handover_id is null;

-- Status history with the rider's location, for tracking and disputes.
create table delivery.delivery_job_events (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  delivery_job_id uuid not null references delivery.delivery_jobs (id) on delete cascade,
  status text not null
    check (status in ('ready', 'assigned', 'picked_up', 'location_update', 'delivered', 'failed', 'returned', 'cancelled')),
  latitude numeric(9,6),
  longitude numeric(9,6),
  note text,
  recorded_by_user_id uuid references core.users (id),
  occurred_at timestamptz not null default now(),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('delivery.delivery_job_events');
create index delivery_job_events_job_index on delivery.delivery_job_events (delivery_job_id, occurred_at);
