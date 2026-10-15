-- Module: promotions
-- Migration 0001: promotions with schedules and budgets, and a record of each time one was used.

create schema promotions;

create table promotions.promotions (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  name text not null,
  description text,
  promotion_type text not null
    check (promotion_type in ('percent_off', 'amount_off', 'buy_x_get_y', 'bundle_price', 'happy_hour', 'loyalty_multiplier')),
  -- What qualifies and what is given, shape depends on promotion_type, for example
  -- {"product_ids": [...], "buy_quantity": 2, "free_quantity": 1} or {"percent_off": 10, "category_ids": [...]}.
  rules jsonb not null default '{}'::jsonb,
  -- Code the customer must give at the till; null means applied automatically.
  coupon_code text,
  starts_at timestamptz,
  ends_at timestamptz,
  -- Branches where it applies; null means every branch.
  branch_ids uuid[],
  -- Customer groups it is limited to; null means every customer.
  customer_group_ids uuid[],
  -- ISO weekdays it runs on, 1 = Monday to 7 = Sunday; null means every day.
  days_of_week integer[] check (days_of_week <@ array[1, 2, 3, 4, 5, 6, 7]),
  -- Daily time window, for example 17:00 to 19:00 for happy hour; null means all day.
  daily_start_time time,
  daily_end_time time,
  is_active boolean not null default true,
  -- Higher priority is tried first when several promotions match.
  priority integer not null default 0,
  can_combine_with_other_promotions boolean not null default false,
  -- Stop once total discount given reaches this; null means no limit.
  budget_limit_amount numeric(18,2) check (budget_limit_amount > 0),
  -- Running total of discount given, compared against budget_limit_amount.
  discount_given_amount numeric(18,2) not null default 0,
  maximum_uses_per_customer integer check (maximum_uses_per_customer > 0),
  created_by_user_id uuid references core.users (id),
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (ends_at is null or starts_at is null or ends_at > starts_at),
  check ((daily_start_time is null) = (daily_end_time is null))
);
select core.prepare_table('promotions.promotions');
create unique index promotions_coupon_code_unique_index on promotions.promotions (organization_id, upper(coupon_code)) where coupon_code is not null and deleted_at is null;
create index promotions_active_index on promotions.promotions (organization_id, priority desc) where is_active and deleted_at is null;

-- Each time a promotion was applied to a sale.
create table promotions.promotion_redemptions (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  promotion_id uuid not null references promotions.promotions (id),
  sale_id uuid not null references point_of_sale.sales (id) on delete cascade,
  -- The line the discount sits on, when it applies to one line rather than the whole sale.
  sale_line_id uuid references point_of_sale.sale_lines (id) on delete cascade,
  customer_id uuid references customers.customers (id),
  discount_amount numeric(18,2) not null default 0 check (discount_amount >= 0),
  -- Extra loyalty points given by a loyalty_multiplier promotion.
  bonus_loyalty_points numeric(18,3) not null default 0,
  redeemed_at timestamptz not null default now(),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('promotions.promotion_redemptions');
create index promotion_redemptions_promotion_index on promotions.promotion_redemptions (promotion_id, redeemed_at desc);
create index promotion_redemptions_sale_index on promotions.promotion_redemptions (sale_id);
create index promotion_redemptions_customer_index on promotions.promotion_redemptions (promotion_id, customer_id) where customer_id is not null;
