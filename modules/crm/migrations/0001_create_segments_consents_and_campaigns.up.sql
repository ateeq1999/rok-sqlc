-- Module: crm
-- Migration 0001: customer segments, marketing consent and text message / WhatsApp campaigns.
-- Only customers with a current marketing consent for the campaign's channel may receive a campaign
-- message (Personal Data Protection Act, 2022); campaign_recipients requires the consent row.

create schema crm;

create table crm.customer_segments (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  name text not null,
  description text,
  -- {"all": [{"field": "days_since_last_visit", "comparison": "greater_than", "value": 30}, {"field": "total_spent_amount", "comparison": "greater_than", "value": 200000}]}
  rule jsonb not null default '{}'::jsonb,
  -- Automatic segments are refreshed from the rule; manual segments keep a hand-picked list.
  is_automatic boolean not null default true,
  -- Count at the last refresh, for quick display; recount before sending.
  member_count_cached integer not null default 0,
  refreshed_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('crm.customer_segments');
create unique index customer_segments_name_unique_index on crm.customer_segments (organization_id, lower(name)) where deleted_at is null;

create table crm.marketing_consents (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  customer_id uuid not null references customers.customers (id) on delete cascade,
  channel text not null check (channel in ('text_message', 'whatsapp', 'email', 'phone_call')),
  consented boolean not null,
  consented_at timestamptz,
  withdrawn_at timestamptz,
  -- Where consent was given or withdrawn: 'till_prompt', 'online_shop_checkout', 'reply_stop', 'paper_form' ...
  source text not null,
  recorded_by_user_id uuid references core.users (id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (not consented or consented_at is not null)
);
select core.prepare_table('crm.marketing_consents');
create unique index marketing_consents_customer_channel_unique_index on crm.marketing_consents (customer_id, channel) where deleted_at is null;
create index marketing_consents_active_index on crm.marketing_consents (organization_id, channel) where consented and withdrawn_at is null and deleted_at is null;

create table crm.campaigns (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  name text not null,
  segment_id uuid references crm.customer_segments (id),
  channel text not null check (channel in ('text_message', 'whatsapp')),
  message_body text not null,
  -- Optional offer attached to the message: {"discount_percent": 10, "promotion_id": "...", "valid_until_on": "2026-12-31"}
  offer jsonb not null default '{}'::jsonb,
  scheduled_at timestamptz,
  sent_at timestamptz,
  status text not null default 'draft' check (status in ('draft', 'scheduled', 'sending', 'sent', 'cancelled')),
  cost_per_message_amount numeric(18,2) not null default 0,
  estimated_cost_amount numeric(18,2) not null default 0,
  created_by_user_id uuid references core.users (id),
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('crm.campaigns');
create index campaigns_status_index on crm.campaigns (organization_id, status);
create index campaigns_scheduled_index on crm.campaigns (scheduled_at) where status = 'scheduled';

create table crm.campaign_recipients (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  campaign_id uuid not null references crm.campaigns (id) on delete cascade,
  customer_id uuid not null references customers.customers (id) on delete cascade,
  -- The consent that allows this message; the sender must re-check it is still active right before sending.
  marketing_consent_id uuid not null references crm.marketing_consents (id),
  phone_number text not null,
  status text not null default 'queued' check (status in ('queued', 'sent', 'delivered', 'failed', 'opted_out')),
  sent_at timestamptz,
  failure_reason text,
  -- First purchase after the message, used to measure the campaign.
  returned_and_bought_at timestamptz,
  attributed_sales_amount numeric(18,2) not null default 0,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (campaign_id, customer_id)
);
select core.prepare_table('crm.campaign_recipients');
create index campaign_recipients_customer_index on crm.campaign_recipients (customer_id);
create index campaign_recipients_queued_index on crm.campaign_recipients (campaign_id) where status = 'queued';
