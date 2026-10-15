-- Module: core
-- Migration 0002: businesses, branches, devices, people and role-based permissions.

create table core.organizations (
  id uuid primary key default core.generate_uuid_v7(),
  name text not null,
  legal_name text,
  business_type text not null default 'shop'
    check (business_type in ('shop', 'supermarket', 'pharmacy', 'fashion', 'cafe', 'restaurant', 'hardware', 'wholesale', 'salon', 'bar', 'bakery', 'butchery', 'electronics', 'supplier', 'other')),
  tax_identification_number text,
  vat_registration_number text,
  country_code text not null default 'TZ',
  currency_code text not null default 'TZS',
  default_language text not null default 'sw',
  time_zone text not null default 'Africa/Dar_es_Salaam',
  logo_attachment_id uuid,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('core.organizations', false);
alter table core.organizations enable row level security;
create policy organization_isolation on core.organizations
  using (id = core.current_organization_id()) with check (id = core.current_organization_id());

create table core.branches (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  name text not null,
  code text not null,
  address text,
  phone_number text,
  business_type text,
  is_active boolean not null default true,
  opening_hours jsonb not null default '{}'::jsonb,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, code)
);
select core.prepare_table('core.branches');

create table core.devices (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid not null references core.branches (id),
  name text not null,
  device_type text not null
    check (device_type in ('register', 'customer_display', 'kitchen_display', 'receipt_printer', 'label_printer', 'scale', 'fiscal_device', 'phone', 'tablet', 'barcode_scanner', 'cash_drawer')),
  connection_details jsonb not null default '{}'::jsonb,
  paired_to_device_id uuid references core.devices (id),
  last_seen_at timestamptz,
  is_active boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('core.devices');
create index devices_branch_index on core.devices (branch_id);

create table core.users (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  full_name text not null,
  email text,
  phone_number text,
  password_hash text,
  personal_identification_number_hash text,
  preferred_language text,
  is_owner boolean not null default false,
  is_active boolean not null default true,
  last_signed_in_at timestamptz,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('core.users');
create unique index users_email_unique_index on core.users (organization_id, lower(email)) where email is not null and deleted_at is null;
create unique index users_phone_number_unique_index on core.users (organization_id, phone_number) where phone_number is not null and deleted_at is null;

create table core.user_branch_access (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  user_id uuid not null references core.users (id) on delete cascade,
  branch_id uuid not null references core.branches (id) on delete cascade,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (user_id, branch_id)
);
select core.prepare_table('core.user_branch_access');

-- The catalogue of permissions is shared by all businesses. Each module inserts its own
-- permissions when it is installed (see module.toml [[permissions]]).
create table core.permissions (
  key text primary key,
  module_key text not null,
  description text not null,
  risk_level text not null default 'normal' check (risk_level in ('normal', 'sensitive', 'money')),
  created_at timestamptz not null default now()
);

create table core.roles (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  key text not null,
  name text not null,
  description text,
  is_system_role boolean not null default false,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, key)
);
select core.prepare_table('core.roles');

create table core.role_permissions (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  role_id uuid not null references core.roles (id) on delete cascade,
  permission_key text not null references core.permissions (key) on delete cascade,
  -- Optional limits, for example {"maximum_discount_percent": 10} or {"maximum_refund_amount": 50000}
  limits jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (role_id, permission_key)
);
select core.prepare_table('core.role_permissions');

create table core.user_roles (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  user_id uuid not null references core.users (id) on delete cascade,
  role_id uuid not null references core.roles (id) on delete cascade,
  branch_id uuid references core.branches (id) on delete cascade,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('core.user_roles');
create unique index user_roles_unique_index on core.user_roles (user_id, role_id, coalesce(branch_id, '00000000-0000-0000-0000-000000000000'::uuid));
