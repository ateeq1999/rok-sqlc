-- Module: core
-- Migration 0005: audit trail, files, notifications and offline sync cursors.

create table core.audit_events (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  branch_id uuid references core.branches (id),
  device_id uuid references core.devices (id),
  user_id uuid references core.users (id),
  approved_by_user_id uuid references core.users (id),
  -- 'sale.void', 'refund.approve', 'price.change', 'module.install', 'assistant.action' ...
  event_key text not null,
  target_table text,
  target_row_id uuid,
  summary text not null,
  details jsonb not null default '{}'::jsonb,
  occurred_at timestamptz not null default now()
);
alter table core.audit_events enable row level security;
create policy organization_isolation on core.audit_events
  using (organization_id = core.current_organization_id()) with check (organization_id = core.current_organization_id());
create index audit_events_lookup_index on core.audit_events (organization_id, occurred_at desc);
create index audit_events_target_index on core.audit_events (target_table, target_row_id);

create table core.attachments (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  file_name text not null,
  content_type text not null,
  size_bytes bigint not null,
  storage_key text not null,
  checksum_sha256 text,
  target_table text,
  target_row_id uuid,
  uploaded_by_user_id uuid references core.users (id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('core.attachments');
create index attachments_target_index on core.attachments (target_table, target_row_id);

create table core.notifications (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  user_id uuid not null references core.users (id) on delete cascade,
  title text not null,
  body text,
  link_screen_key text,
  link_row_id uuid,
  channel text not null default 'in_app' check (channel in ('in_app', 'push', 'text_message', 'email', 'whatsapp')),
  read_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('core.notifications');
create index notifications_unread_index on core.notifications (user_id) where read_at is null;

-- Where each device is in core.change_log, so it pulls only what it has not seen.
create table core.device_sync_cursors (
  device_id uuid primary key references core.devices (id) on delete cascade,
  organization_id uuid not null references core.organizations (id) on delete cascade,
  last_pulled_sequence_number bigint not null default 0,
  last_pushed_at timestamptz,
  last_pulled_at timestamptz,
  pending_upload_count integer not null default 0
);
alter table core.device_sync_cursors enable row level security;
create policy organization_isolation on core.device_sync_cursors
  using (organization_id = core.current_organization_id()) with check (organization_id = core.current_organization_id());

-- Changes a device made offline that the server could not apply automatically.
create table core.sync_conflicts (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  device_id uuid not null references core.devices (id),
  table_name text not null,
  row_id uuid not null,
  device_row jsonb not null,
  server_row jsonb not null,
  resolution text check (resolution in ('kept_server', 'kept_device', 'merged')),
  resolved_by_user_id uuid references core.users (id),
  resolved_at timestamptz,
  created_at timestamptz not null default now()
);
alter table core.sync_conflicts enable row level security;
create policy organization_isolation on core.sync_conflicts
  using (organization_id = core.current_organization_id()) with check (organization_id = core.current_organization_id());
