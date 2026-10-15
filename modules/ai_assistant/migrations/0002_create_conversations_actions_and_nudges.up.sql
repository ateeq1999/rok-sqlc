-- Module: ai_assistant
-- Migration 0002: chats, the drafts Msaidizi proposes, and nudge history for frequency limits.

create table ai_assistant.conversations (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  user_id uuid not null references core.users (id) on delete cascade,
  branch_id uuid references core.branches (id),
  -- Page the chat was opened from, so answers can use that page's context.
  page_key text,
  title text,
  channel text not null default 'floating_panel' check (channel in ('floating_panel', 'full_chat', 'whatsapp')),
  last_message_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('ai_assistant.conversations');
create index conversations_user_index on ai_assistant.conversations (user_id, last_message_at desc);

create table ai_assistant.messages (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  conversation_id uuid not null references ai_assistant.conversations (id) on delete cascade,
  role text not null check (role in ('user', 'assistant', 'tool')),
  content text not null,
  -- Records the answer was based on: [{"table": "point_of_sale.sales", "row_id": "...", "label": "S-4813"}]
  cited_sources jsonb not null default '[]'::jsonb,
  input_token_count integer,
  output_token_count integer,
  model_name text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('ai_assistant.messages');
create index messages_conversation_index on ai_assistant.messages (conversation_id, created_at);
-- For the monthly question limit.
create index messages_usage_index on ai_assistant.messages (organization_id, created_at) where role = 'user';

-- A draft the assistant prepared (restock order, price change...). Nothing happens until a person confirms.
create table ai_assistant.proposed_actions (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  conversation_id uuid references ai_assistant.conversations (id) on delete cascade,
  message_id uuid references ai_assistant.messages (id) on delete set null,
  action_key text not null,
  target_page_key text,
  draft_payload jsonb not null default '{}'::jsonb,
  status text not null default 'proposed' check (status in ('proposed', 'confirmed', 'rejected', 'expired', 'failed')),
  -- The confirming user must also hold this permission, for example purchasing.orders.create.
  requires_permission_key text references core.permissions (key),
  confirmed_by_user_id uuid references core.users (id),
  confirmed_at timestamptz,
  expires_at timestamptz,
  failure_message text,
  -- The record created when the draft was confirmed, for example purchasing.purchase_orders + its id.
  result_table text,
  result_row_id uuid,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (status <> 'confirmed' or confirmed_by_user_id is not null)
);
select core.prepare_table('ai_assistant.proposed_actions');
create index proposed_actions_conversation_index on ai_assistant.proposed_actions (conversation_id);
create index proposed_actions_open_index on ai_assistant.proposed_actions (organization_id, created_at) where status = 'proposed';

-- Each time a nudge was shown, used to respect the rule frequency and the daily limit per person.
create table ai_assistant.nudge_impressions (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  page_rule_id uuid not null references ai_assistant.page_rules (id) on delete cascade,
  user_id uuid not null references core.users (id) on delete cascade,
  branch_id uuid references core.branches (id),
  shown_message text,
  shown_at timestamptz not null default now(),
  outcome text not null default 'shown' check (outcome in ('shown', 'clicked_action', 'dismissed', 'ignored')),
  clicked_action_key text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (outcome <> 'clicked_action' or clicked_action_key is not null)
);
select core.prepare_table('ai_assistant.nudge_impressions');
create index nudge_impressions_frequency_index on ai_assistant.nudge_impressions (user_id, page_rule_id, shown_at desc);
create index nudge_impressions_daily_index on ai_assistant.nudge_impressions (organization_id, user_id, shown_at desc);
