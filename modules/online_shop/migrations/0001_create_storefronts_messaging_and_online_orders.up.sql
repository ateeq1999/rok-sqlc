-- Module: online_shop
-- Migration 0001: web storefronts, WhatsApp and text message ordering, and online orders.

create schema online_shop;

create table online_shop.storefronts (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  -- The branch that packs and delivers this storefront's orders.
  branch_id uuid not null references core.branches (id),
  -- <subdomain_name>.rokpos shop address; unique across every business.
  subdomain_name text not null check (subdomain_name ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
  display_name text not null,
  description text,
  logo_attachment_id uuid references core.attachments (id),
  is_open boolean not null default false,
  delivery_fee_amount numeric(18,2) not null default 0 check (delivery_fee_amount >= 0),
  minimum_order_amount numeric(18,2) not null default 0 check (minimum_order_amount >= 0),
  -- [{"name": "Mikocheni", "delivery_fee_amount": 3000}, {"name": "Kariakoo", "delivery_fee_amount": 5000}]
  delivery_areas jsonb not null default '[]'::jsonb,
  pickup_enabled boolean not null default true,
  -- Colours, banner and layout choices: {"primary_color": "#0a7d5a", "banner_attachment_id": "..."}
  theme jsonb not null default '{}'::jsonb,
  whatsapp_phone_number text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('online_shop.storefronts');
create unique index storefronts_subdomain_unique_index on online_shop.storefronts (subdomain_name) where deleted_at is null;

create table online_shop.messaging_channels (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  storefront_id uuid references online_shop.storefronts (id),
  channel_type text not null check (channel_type in ('whatsapp_business', 'text_message', 'web_chat')),
  phone_number text,
  provider_name text,
  -- Name of the secret in the server's secret store. Never put API keys or tokens here in plain text.
  credentials_reference text,
  is_active boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('online_shop.messaging_channels');

-- Customer messages such as "nataka mchele 5kg na mafuta 2 lita", read by the assistant and turned into orders.
create table online_shop.inbound_messages (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  messaging_channel_id uuid not null references online_shop.messaging_channels (id),
  sender_phone_number text not null,
  -- Matched by phone number when the sender is a known customer.
  customer_id uuid references customers.customers (id),
  body text,
  -- Provider's message id, used to ignore duplicate webhook deliveries.
  provider_message_reference text,
  received_at timestamptz not null default now(),
  parsed_by_assistant_at timestamptz,
  -- Order the assistant read from the message: {"lines": [{"product_id": "...", "quantity": 5}], "delivery_address": "..."}
  parsed_order jsonb,
  status text not null default 'new' check (status in ('new', 'turned_into_order', 'replied', 'ignored')),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('online_shop.inbound_messages');
create index inbound_messages_channel_index on online_shop.inbound_messages (messaging_channel_id, received_at desc);
create index inbound_messages_new_index on online_shop.inbound_messages (organization_id) where status = 'new' and deleted_at is null;
create unique index inbound_messages_provider_reference_unique_index on online_shop.inbound_messages (messaging_channel_id, provider_message_reference)
  where provider_message_reference is not null;

create table online_shop.online_orders (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  order_number text not null,
  storefront_id uuid references online_shop.storefronts (id),
  -- Branch that fulfils the order.
  branch_id uuid not null references core.branches (id),
  channel text not null check (channel in ('web', 'whatsapp', 'phone')),
  customer_id uuid references customers.customers (id),
  customer_name text,
  customer_phone_number text,
  inbound_message_id uuid references online_shop.inbound_messages (id),
  status text not null default 'new'
    check (status in ('new', 'waiting_for_payment', 'paid', 'packing', 'ready_for_pickup', 'with_rider', 'delivered', 'cancelled')),
  fulfilment text not null default 'delivery' check (fulfilment in ('delivery', 'pickup')),
  delivery_address text,
  delivery_area_name text,
  delivery_fee_amount numeric(18,2) not null default 0,
  subtotal_amount numeric(18,2) not null default 0,
  discount_amount numeric(18,2) not null default 0,
  tax_amount numeric(18,2) not null default 0,
  total_amount numeric(18,2) not null default 0,
  payment_status text not null default 'unpaid'
    check (payment_status in ('unpaid', 'partly_paid', 'paid', 'cash_on_delivery', 'refunded')),
  payment_link text,
  -- Stock is reserved for the order until this time, then released if unpaid.
  stock_held_until timestamptz,
  -- point_of_sale.sales id once the order is rung up; no foreign key because point_of_sale is optional.
  sale_id uuid,
  packed_by_user_id uuid references core.users (id),
  customer_note text,
  cancelled_reason text,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, order_number),
  check (fulfilment = 'pickup' or delivery_address is not null or status in ('new', 'cancelled'))
);
select core.prepare_table('online_shop.online_orders');
create index online_orders_status_index on online_shop.online_orders (branch_id, status) where deleted_at is null;
create index online_orders_customer_index on online_shop.online_orders (customer_id);
create index online_orders_stock_hold_index on online_shop.online_orders (stock_held_until) where stock_held_until is not null and payment_status = 'unpaid';

create table online_shop.online_order_lines (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  online_order_id uuid not null references online_shop.online_orders (id) on delete cascade,
  product_id uuid not null references catalog.products (id),
  product_variant_id uuid references catalog.product_variants (id),
  description text not null,
  quantity numeric(18,3) not null check (quantity > 0),
  unit_price numeric(18,2) not null check (unit_price >= 0),
  discount_amount numeric(18,2) not null default 0,
  tax_rate_percent numeric(6,3) not null default 0,
  line_total_amount numeric(18,2) not null default 0,
  -- Packed quantity can be lower when an item is out of stock.
  packed_quantity numeric(18,3),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('online_shop.online_order_lines');
create index online_order_lines_order_index on online_shop.online_order_lines (online_order_id);
create index online_order_lines_product_index on online_shop.online_order_lines (product_id);
