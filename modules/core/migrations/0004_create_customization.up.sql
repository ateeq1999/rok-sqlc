-- Module: core
-- Migration 0004: everything a business can change without code (Studio).
-- Core tables are never altered by customization: extra fields live in each table's
-- custom_fields jsonb column and are described here, so upgrades stay safe.

-- Settings with scopes: a value for the whole business, overridden per branch or per device.
create table core.settings (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  module_key text not null,
  key text not null,
  value jsonb not null,
  branch_id uuid references core.branches (id) on delete cascade,
  device_id uuid references core.devices (id) on delete cascade,
  updated_by_user_id uuid references core.users (id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('core.settings');
create unique index settings_scope_unique_index on core.settings (
  organization_id, module_key, key,
  coalesce(branch_id, '00000000-0000-0000-0000-000000000000'::uuid),
  coalesce(device_id, '00000000-0000-0000-0000-000000000000'::uuid)
);

create table core.custom_field_definitions (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  -- The table being extended, for example 'catalog.products' or 'hr.employees'.
  target_table text not null,
  field_key text not null check (field_key ~ '^[a-z][a-z0-9_]{1,62}$'),
  label text not null,
  label_translations jsonb not null default '{}'::jsonb,
  field_type text not null
    check (field_type in ('text', 'long_text', 'number', 'money', 'date', 'date_time', 'choice', 'multiple_choice', 'yes_no', 'record_link', 'file', 'phone_number', 'email')),
  -- Choices, linked table, number of decimals, default value...
  options jsonb not null default '{}'::jsonb,
  is_required boolean not null default false,
  is_searchable boolean not null default false,
  show_on_receipt boolean not null default false,
  show_on_label boolean not null default false,
  readable_by_assistant boolean not null default true,
  editable_by_role_keys text[] not null default '{}',
  branch_id uuid references core.branches (id) on delete cascade,
  sort_order integer not null default 0,
  is_active boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, target_table, field_key)
);
select core.prepare_table('core.custom_field_definitions');

-- Where fields sit on each screen (form sections, register buttons, receipt blocks).
create table core.screen_layouts (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  screen_key text not null,
  branch_id uuid references core.branches (id) on delete cascade,
  role_key text,
  layout jsonb not null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('core.screen_layouts');
create unique index screen_layouts_scope_unique_index on core.screen_layouts (
  organization_id, screen_key,
  coalesce(branch_id, '00000000-0000-0000-0000-000000000000'::uuid),
  coalesce(role_key, '')
);

-- Rules such as "discount over 10% needs a manager" or "purchase order over 1,000,000 needs the owner".
create table core.approval_rules (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  action_key text not null,
  name text not null,
  -- {"field": "total_amount", "comparison": "greater_than", "value": 1000000}
  condition jsonb not null default '{}'::jsonb,
  approver_role_keys text[] not null,
  approval_method text not null default 'personal_identification_number'
    check (approval_method in ('personal_identification_number', 'in_app', 'text_message')),
  is_active boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('core.approval_rules');

-- Document numbers: S-4813, PO-0412, INV-0318, per business and optionally per branch.
create table core.number_sequences (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  sequence_key text not null,
  branch_id uuid references core.branches (id) on delete cascade,
  prefix text not null default '',
  padding_length integer not null default 4,
  next_value bigint not null default 1,
  reset_period text not null default 'never' check (reset_period in ('never', 'yearly', 'monthly')),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('core.number_sequences');
create unique index number_sequences_scope_unique_index on core.number_sequences (
  organization_id, sequence_key, coalesce(branch_id, '00000000-0000-0000-0000-000000000000'::uuid)
);

-- Returns the next formatted number, safe under concurrent use.
create function core.take_next_number(target_organization_id uuid, target_sequence_key text, target_branch_id uuid default null)
returns text language plpgsql as $$
declare
  taken_value bigint;
  sequence_prefix text;
  sequence_padding integer;
begin
  update core.number_sequences
     set next_value = next_value + 1
   where organization_id = target_organization_id
     and sequence_key = target_sequence_key
     and branch_id is not distinct from target_branch_id
  returning next_value - 1, prefix, padding_length into taken_value, sequence_prefix, sequence_padding;
  if taken_value is null then
    raise exception 'No number sequence % for this business', target_sequence_key;
  end if;
  return sequence_prefix || lpad(taken_value::text, sequence_padding, '0');
end;
$$;

-- Every Studio publish is a version that can be rolled back or exported as a module package.
create table core.studio_publications (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  version_number integer not null,
  summary text not null,
  snapshot jsonb not null,
  published_by_user_id uuid references core.users (id),
  published_at timestamptz not null default now(),
  unique (organization_id, version_number)
);
alter table core.studio_publications enable row level security;
create policy organization_isolation on core.studio_publications
  using (organization_id = core.current_organization_id()) with check (organization_id = core.current_organization_id());

-- Text a business can rewrite: receipt footers, menu names, assistant messages.
create table core.translations (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  text_key text not null,
  language_code text not null,
  text_value text not null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, text_key, language_code)
);
select core.prepare_table('core.translations');
