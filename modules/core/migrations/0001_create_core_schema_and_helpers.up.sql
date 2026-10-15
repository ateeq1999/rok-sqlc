-- Module: core
-- Migration 0001: the core schema, the change log and the helper functions every module uses.
--
-- Conventions used by every module (read this once):
--   * Each module owns one PostgreSQL schema named after its key (core, catalog, hr, ...).
--     Installing a module creates its schema; uninstalling archives the schema.
--   * Primary keys are time-ordered UUIDs (version 7) so devices can create rows offline.
--   * Every business table has organization_id and is isolated per business with row level security.
--   * Every business table has created_at, updated_at, deleted_at (soft delete) and row_version
--     (incremented on each update, used by offline sync to detect conflicts).
--   * Tables a business may extend have custom_fields jsonb, described by core.custom_field_definitions.
--   * Money is numeric(18,2); quantities are numeric(18,3) so weights and metres work.
--   * Status columns are text with a check constraint instead of PostgreSQL enums,
--     so a later migration can add a value without rewriting types.
--   * After creating a table, call core.prepare_table('schema.table') to add the triggers and policy.

create schema core;

-- Time-ordered UUID (RFC 9562 version 7). Native uuidv7() arrives in PostgreSQL 18.
create function core.generate_uuid_v7() returns uuid
language plpgsql volatile as $$
declare
  unix_time_milliseconds bigint := floor(extract(epoch from clock_timestamp()) * 1000);
  uuid_bytes bytea := uuid_send(gen_random_uuid());
begin
  uuid_bytes := overlay(uuid_bytes placing substring(int8send(unix_time_milliseconds) from 3) from 1 for 6);
  uuid_bytes := set_byte(uuid_bytes, 6, (b'0111' || get_byte(uuid_bytes, 6)::bit(4))::bit(8)::int);
  return encode(uuid_bytes, 'hex')::uuid;
end;
$$;

-- The business the current database session acts for. The API sets it per request:
--   select set_config('app.organization_id', '<uuid>', true);
create function core.current_organization_id() returns uuid
language sql stable as $$
  select nullif(current_setting('app.organization_id', true), '')::uuid;
$$;

-- The signed-in user, set the same way with 'app.user_id'. Used by audit triggers.
create function core.current_user_id() returns uuid
language sql stable as $$
  select nullif(current_setting('app.user_id', true), '')::uuid;
$$;

-- Every insert, update and delete on a prepared table is written here.
-- Devices pull changes after their last seen sequence number to sync offline copies.
create table core.change_log (
  sequence_number bigint generated always as identity primary key,
  organization_id uuid,
  table_name text not null,
  row_id uuid not null,
  operation text not null check (operation in ('insert', 'update', 'delete')),
  row_version bigint,
  changed_by_user_id uuid,
  changed_at timestamptz not null default now()
);
create index change_log_organization_sequence_index on core.change_log (organization_id, sequence_number);

create function core.touch_row() returns trigger
language plpgsql as $$
begin
  new.updated_at := now();
  new.row_version := coalesce(old.row_version, 0) + 1;
  return new;
end;
$$;

create function core.record_change() returns trigger
language plpgsql as $$
declare
  changed_row jsonb := to_jsonb(coalesce(new, old));
begin
  insert into core.change_log (organization_id, table_name, row_id, operation, row_version, changed_by_user_id)
  values (
    (changed_row ->> 'organization_id')::uuid,
    tg_table_schema || '.' || tg_table_name,
    (changed_row ->> 'id')::uuid,
    lower(tg_op),
    (changed_row ->> 'row_version')::bigint,
    core.current_user_id()
  );
  return null;
end;
$$;

-- Adds the standard triggers and, for business tables, the per-business isolation policy.
create function core.prepare_table(target_table regclass, isolate_by_organization boolean default true) returns void
language plpgsql as $$
begin
  execute format('create trigger touch_row before update on %s for each row execute function core.touch_row()', target_table);
  execute format('create trigger record_change after insert or update or delete on %s for each row execute function core.record_change()', target_table);
  if isolate_by_organization then
    execute format('alter table %s enable row level security', target_table);
    execute format(
      'create policy organization_isolation on %s using (organization_id = core.current_organization_id()) with check (organization_id = core.current_organization_id())',
      target_table
    );
  end if;
end;
$$;
