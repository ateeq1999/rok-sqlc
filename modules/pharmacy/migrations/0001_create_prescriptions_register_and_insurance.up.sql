-- Module: pharmacy
-- Migration 0001: prescribers, prescriptions, the controlled substance register and insurance claims.

create schema pharmacy;

-- Doctors, clinical officers and dentists who write prescriptions.
create table pharmacy.prescribers (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  name text not null,
  -- Professional council registration number (e.g. Medical Council of Tanganyika).
  registration_number text,
  facility text,
  phone_number text,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('pharmacy.prescribers');
create index prescribers_registration_number_index on pharmacy.prescribers (organization_id, registration_number);

create table pharmacy.prescriptions (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid not null references core.branches (id),
  prescription_number text,
  customer_id uuid references customers.customers (id) on delete set null,
  -- When the patient is not a saved customer.
  patient_name text,
  prescriber_id uuid references pharmacy.prescribers (id) on delete set null,
  written_on date,
  -- Photo or scan of the paper prescription.
  attachment_id uuid references core.attachments (id) on delete set null,
  status text not null default 'received'
    check (status in ('received', 'dispensed', 'partly_dispensed', 'refused')),
  refusal_reason text,
  dispensed_by_user_id uuid references core.users (id),
  dispensed_at timestamptz,
  sale_id uuid references point_of_sale.sales (id) on delete set null,
  notes text,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('pharmacy.prescriptions');
create index prescriptions_customer_index on pharmacy.prescriptions (customer_id);
create index prescriptions_status_index on pharmacy.prescriptions (organization_id, status, created_at);
create index prescriptions_sale_index on pharmacy.prescriptions (sale_id);

create table pharmacy.prescription_items (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  prescription_id uuid not null references pharmacy.prescriptions (id) on delete cascade,
  product_id uuid references catalog.products (id),
  -- As written, when the product is not stocked or is substituted.
  prescribed_item_text text,
  dosage_instructions text,
  quantity_prescribed numeric(18,3) not null check (quantity_prescribed > 0),
  quantity_dispensed numeric(18,3) not null default 0 check (quantity_dispensed >= 0),
  batch_id uuid references inventory.stock_batches (id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (product_id is not null or prescribed_item_text is not null)
);
select core.prepare_table('pharmacy.prescription_items');
create index prescription_items_prescription_index on pharmacy.prescription_items (prescription_id);

-- The dangerous drugs register: an append-only ledger per controlled product with running balance.
create table pharmacy.controlled_substance_entries (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid not null references core.branches (id),
  product_id uuid not null references catalog.products (id),
  batch_id uuid references inventory.stock_batches (id),
  entry_type text not null check (entry_type in ('received', 'dispensed', 'destroyed', 'returned')),
  quantity numeric(18,3) not null check (quantity > 0),
  -- Register balance for this product and branch after this entry.
  balance_after numeric(18,3) not null,
  prescription_id uuid references pharmacy.prescriptions (id),
  -- Supplier, patient or destruction certificate reference as written in the paper register.
  counterparty_reference text,
  witnessed_by_user_id uuid references core.users (id),
  recorded_by_user_id uuid not null references core.users (id),
  recorded_at timestamptz not null default now(),
  notes text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('pharmacy.controlled_substance_entries');
create index controlled_substance_entries_product_index on pharmacy.controlled_substance_entries (branch_id, product_id, recorded_at);

-- NHIF, Jubilee, Strategis, AAR and other medical insurers.
create table pharmacy.insurance_providers (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  name text not null,
  provider_code text,
  -- Share of the bill the patient pays themselves.
  patient_copayment_percent numeric(6,3) not null default 0,
  claim_submission_notes text,
  is_active boolean not null default true,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('pharmacy.insurance_providers');

create table pharmacy.insurance_claims (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  claim_number text,
  sale_id uuid not null references point_of_sale.sales (id),
  insurance_provider_id uuid not null references pharmacy.insurance_providers (id),
  prescription_id uuid references pharmacy.prescriptions (id) on delete set null,
  member_number text not null,
  member_name text,
  claim_amount numeric(18,2) not null check (claim_amount >= 0),
  approved_amount numeric(18,2),
  paid_amount numeric(18,2) not null default 0,
  status text not null default 'submitted' check (status in ('submitted', 'approved', 'rejected', 'paid')),
  rejection_reason text,
  submitted_at timestamptz not null default now(),
  paid_at timestamptz,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('pharmacy.insurance_claims');
create index insurance_claims_provider_status_index on pharmacy.insurance_claims (insurance_provider_id, status);
create index insurance_claims_sale_index on pharmacy.insurance_claims (sale_id);
