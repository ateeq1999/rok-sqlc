-- Module: core
-- Migration 0003: the module registry behind one-click install.
--
-- Two levels:
--   1. installed_modules / applied_migrations: the module's schema exists in this database.
--      Happens once per database (server or on-premise box), run by the installer.
--   2. organization_modules: a business has switched the module on, for some or all branches.
--      This is the "Install" button a shop owner presses. It seeds that business's defaults
--      (roles, settings, assistant rules) and never touches other businesses.

create table core.installed_modules (
  module_key text primary key,
  display_name text not null,
  installed_version text not null,
  schema_name text not null unique,
  status text not null default 'installed'
    check (status in ('installing', 'installed', 'upgrading', 'failed', 'archived')),
  manifest jsonb not null,
  installed_at timestamptz not null default now(),
  upgraded_at timestamptz,
  archived_at timestamptz,
  archived_schema_name text
);

create table core.applied_migrations (
  module_key text not null,
  migration_name text not null,
  checksum text not null,
  applied_at timestamptz not null default now(),
  duration_milliseconds integer not null,
  primary key (module_key, migration_name)
);

create table core.module_dependencies (
  module_key text not null references core.installed_modules (module_key) on delete cascade,
  depends_on_module_key text not null,
  is_optional boolean not null default false,
  minimum_version text,
  primary key (module_key, depends_on_module_key)
);

create table core.organization_modules (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  module_key text not null references core.installed_modules (module_key),
  status text not null default 'active'
    check (status in ('activating', 'active', 'paused', 'deactivated', 'failed')),
  -- Null means every branch.
  enabled_branch_ids uuid[],
  activated_by_user_id uuid references core.users (id),
  activated_at timestamptz,
  deactivated_at timestamptz,
  -- Data stays for this long after deactivation, then a cleanup job may export and delete it.
  keep_data_until date,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, module_key)
);
select core.prepare_table('core.organization_modules');

-- One row per click on Install, Upgrade or Uninstall, so the screen can show progress
-- and the installer can resume or roll back after a crash.
create table core.module_jobs (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid references core.organizations (id) on delete cascade,
  module_key text not null,
  job_type text not null check (job_type in ('install', 'activate', 'upgrade', 'deactivate', 'archive')),
  target_version text,
  status text not null default 'queued' check (status in ('queued', 'running', 'succeeded', 'failed', 'rolled_back')),
  -- [{"key": "create_tables", "label": "Create tables", "status": "running", "details": [...]}]
  steps jsonb not null default '[]'::jsonb,
  backup_reference text,
  error_message text,
  requested_by_user_id uuid references core.users (id),
  started_at timestamptz,
  finished_at timestamptz,
  created_at timestamptz not null default now()
);
create index module_jobs_module_index on core.module_jobs (module_key, created_at desc);
