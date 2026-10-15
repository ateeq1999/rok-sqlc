-- Module: ai_assistant
-- Migration 0001: Msaidizi settings per business and the rules for where the button and nudges appear.

create schema ai_assistant;

-- One row per business: name, language, model and what the assistant is allowed to read.
create table ai_assistant.assistant_settings (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  assistant_name text not null default 'Msaidizi',
  is_enabled boolean not null default true,
  reply_language text not null default 'same_as_user' check (reply_language in ('same_as_user', 'sw', 'en')),
  model_provider text not null default 'anthropic',
  model_name text not null,
  can_read_sales boolean not null default true,
  can_read_costs boolean not null default false,
  can_read_staff_pay boolean not null default false,
  can_read_customer_contacts boolean not null default false,
  button_position text not null default 'bottom_right' check (button_position in ('bottom_right', 'bottom_left')),
  maximum_nudges_per_person_per_day integer not null default 3 check (maximum_nudges_per_person_per_day >= 0),
  -- No nudges while a sale is being rung up at the register.
  quiet_while_sale_open boolean not null default true,
  -- Null means no limit.
  monthly_question_limit integer check (monthly_question_limit >= 0),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id)
);
select core.prepare_table('ai_assistant.assistant_settings');

-- Where the floating button shows and when it may nudge. Modules seed default rules
-- (module.toml [[assistant_rules]]); the owner can change or add their own.
create table ai_assistant.page_rules (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  -- Matches a [[pages]] page_key from a module manifest: inventory, register, payroll ...
  page_key text not null,
  module_key text not null,
  -- Null means every branch.
  branch_id uuid references core.branches (id) on delete cascade,
  show_button boolean not null default true,
  allow_nudges boolean not null default true,
  -- Empty means every role.
  visible_to_role_keys text[] not null default '{}',
  -- Measure the nudge watches, for example items_running_out_within_days. Null means a button-only rule.
  trigger_metric_key text,
  trigger_comparison text not null default 'any' check (trigger_comparison in ('greater_than', 'less_than', 'equals', 'any')),
  trigger_threshold numeric,
  -- "Bidhaa {item_count} zitaisha ndani ya siku {days}" with {placeholders} filled from the metric.
  message_template text,
  -- {"en": "...", "sw": "..."}
  message_translations jsonb not null default '{}'::jsonb,
  -- [{"action_key": "make_restock_order", "label": "Order now", "kind": "make_draft", "target_page_key": "restock_cart"}]
  allowed_actions jsonb not null default '[]'::jsonb check (jsonb_typeof(allowed_actions) = 'array'),
  frequency text not null default 'once_per_day'
    check (frequency in ('every_visit', 'once_per_session', 'once_per_day', 'once_per_week')),
  -- Seeded by a module on activation, so an upgrade may refresh it.
  is_default_rule boolean not null default false,
  is_active boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (trigger_comparison = 'any' or trigger_threshold is not null)
);
select core.prepare_table('ai_assistant.page_rules');
create unique index page_rules_scope_unique_index on ai_assistant.page_rules (
  organization_id, page_key, coalesce(trigger_metric_key, ''),
  coalesce(branch_id, '00000000-0000-0000-0000-000000000000'::uuid)
) where deleted_at is null;
create index page_rules_module_index on ai_assistant.page_rules (organization_id, module_key);
