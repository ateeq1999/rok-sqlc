-- Module: electronics (phones, computers, appliances)
-- Migration 0001: warranties on sold items and the repair workshop.

create schema electronics;

create table electronics.warranties (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  warranty_number text,
  sale_line_id uuid references point_of_sale.sale_lines (id) on delete set null,
  serial_number_id uuid references inventory.stock_serial_numbers (id),
  product_id uuid references catalog.products (id),
  customer_id uuid references customers.customers (id) on delete set null,
  warranty_type text not null default 'shop' check (warranty_type in ('shop', 'extended', 'manufacturer')),
  starts_on date not null,
  ends_on date not null,
  -- What is and is not covered, as printed on the warranty card.
  coverage text,
  -- Price charged for an extended warranty; zero for free shop or manufacturer cover.
  price_amount numeric(18,2) not null default 0,
  is_void boolean not null default false,
  void_reason text,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (ends_on >= starts_on)
);
select core.prepare_table('electronics.warranties');
create index warranties_serial_number_index on electronics.warranties (serial_number_id);
create index warranties_customer_index on electronics.warranties (customer_id);
create index warranties_sale_line_index on electronics.warranties (sale_line_id);

create table electronics.repair_jobs (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid not null references core.branches (id),
  job_number text not null,
  customer_id uuid references customers.customers (id) on delete set null,
  customer_name text,
  customer_phone_number text,
  device_description text not null,
  -- Typed from the device; may not exist in stock records if bought elsewhere.
  serial_number_text text,
  -- Second identifier on dual-SIM phones.
  secondary_serial_number_text text,
  fault_description text not null,
  -- Scratches, cracks, missing screws noted at book-in.
  condition_notes text,
  -- Charger, case, SIM card, memory card left with the device.
  accessories_left text,
  booked_in_by_user_id uuid references core.users (id),
  technician_user_id uuid references core.users (id) on delete set null,
  status text not null default 'booked_in'
    check (status in ('booked_in', 'diagnosing', 'needs_approval', 'waiting_for_part', 'fixing', 'ready_to_collect', 'collected', 'cancelled')),
  quoted_amount numeric(18,2),
  deposit_amount numeric(18,2) not null default 0,
  -- When the customer accepted the quote.
  approved_at timestamptz,
  under_warranty_id uuid references electronics.warranties (id) on delete set null,
  promised_on date,
  collected_at timestamptz,
  sale_id uuid references point_of_sale.sales (id) on delete set null,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('electronics.repair_jobs');
create unique index repair_jobs_job_number_unique_index on electronics.repair_jobs (organization_id, job_number) where deleted_at is null;
create index repair_jobs_status_index on electronics.repair_jobs (branch_id, status);
create index repair_jobs_technician_index on electronics.repair_jobs (technician_user_id, status);
create index repair_jobs_customer_index on electronics.repair_jobs (customer_id);
create index repair_jobs_serial_number_text_index on electronics.repair_jobs (organization_id, serial_number_text);

-- Spare parts and labour charged on a repair job.
create table electronics.repair_job_parts (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  repair_job_id uuid not null references electronics.repair_jobs (id) on delete cascade,
  product_id uuid not null references catalog.products (id),
  serial_number_id uuid references inventory.stock_serial_numbers (id),
  quantity numeric(18,3) not null default 1 check (quantity > 0),
  unit_price_amount numeric(18,2) not null default 0,
  -- Not charged because the job is under warranty.
  is_covered_by_warranty boolean not null default false,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('electronics.repair_job_parts');
create index repair_job_parts_repair_job_index on electronics.repair_job_parts (repair_job_id);

-- Status history and notes; also records when the customer was told (SMS/WhatsApp).
create table electronics.repair_job_events (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  repair_job_id uuid not null references electronics.repair_jobs (id) on delete cascade,
  previous_status text,
  new_status text,
  note text,
  changed_by_user_id uuid references core.users (id),
  notified_customer_at timestamptz,
  notification_channel text check (notification_channel in ('sms', 'whatsapp', 'phone_call', 'in_person')),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('electronics.repair_job_events');
create index repair_job_events_repair_job_index on electronics.repair_job_events (repair_job_id, created_at);
