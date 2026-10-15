-- Module: marketplace
-- Migration 0002: orders between a buyer shop and a vendor, deliveries and ratings.
--
-- These rows are shared by two businesses. organization_id is the VENDOR's organization, so the
-- standard organization_isolation policy from core.prepare_table gives the vendor access.
-- buyer_organization_id plus a buyer_access policy gives the buying shop access to the same rows.
-- Child tables without buyer_organization_id use an EXISTS check on their parent instead.
-- Permissive policies are ORed, so each side sees its own rows and no third business sees any.

create table marketplace.orders (
  id uuid primary key default core.generate_uuid_v7(),
  -- The vendor's organization.
  organization_id uuid not null references core.organizations (id) on delete cascade,
  buyer_organization_id uuid not null references core.organizations (id) on delete cascade,
  vendor_id uuid not null references marketplace.vendors (id),
  -- <group_number>-<n>, unique per buyer, e.g. MK-1121-2.
  order_number text not null,
  -- Shared by every vendor order from one buyer checkout, e.g. MK-1121.
  group_number text not null,
  status text not null default 'placed'
    check (status in ('placed', 'accepted', 'change_suggested', 'packed', 'out_for_delivery', 'delivered', 'received', 'cancelled', 'disputed')),
  payment_method text not null
    check (payment_method in ('trade_credit', 'mobile_money', 'cash_on_delivery', 'invoice')),
  goods_amount numeric(18,2) not null default 0,
  delivery_fee_amount numeric(18,2) not null default 0,
  -- Platform commission charged to the vendor; not added to what the buyer pays.
  commission_amount numeric(18,2) not null default 0,
  total_amount numeric(18,2) not null default 0,
  deliver_to_branch_id uuid not null references core.branches (id),
  deliver_to_stock_location_id uuid references inventory.stock_locations (id),
  -- The buyer's own purchase order this marketplace order fulfils.
  purchase_order_id uuid references purchasing.purchase_orders (id),
  requested_delivery_on date,
  placed_by_user_id uuid references core.users (id),
  approved_by_user_id uuid references core.users (id),
  approved_at timestamptz,
  accepted_at timestamptz,
  cancelled_at timestamptz,
  cancellation_reason text,
  buyer_note text,
  vendor_note text,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (buyer_organization_id, order_number),
  check (organization_id <> buyer_organization_id),
  check (total_amount = goods_amount + delivery_fee_amount)
);
select core.prepare_table('marketplace.orders');
create index orders_vendor_status_index on marketplace.orders (organization_id, status, created_at desc);
create index orders_buyer_index on marketplace.orders (buyer_organization_id, created_at desc);
create index orders_group_index on marketplace.orders (buyer_organization_id, group_number);
create index orders_purchase_order_index on marketplace.orders (purchase_order_id) where purchase_order_id is not null;
-- Second side of a shared row: the buying shop reads and updates (cancel, receive, dispute) its orders.
create policy buyer_access on marketplace.orders
  using (buyer_organization_id = core.current_organization_id())
  with check (buyer_organization_id = core.current_organization_id());

create table marketplace.order_lines (
  id uuid primary key default core.generate_uuid_v7(),
  -- The vendor's organization, same as the order.
  organization_id uuid not null references core.organizations (id) on delete cascade,
  order_id uuid not null references marketplace.orders (id) on delete cascade,
  listing_id uuid not null references marketplace.listings (id),
  -- The buyer's own product that receives the stock (mapped at checkout or on receipt).
  buyer_product_id uuid references catalog.products (id),
  -- Copied from the listing so later listing edits do not change past orders.
  title text not null,
  unit_description text not null,
  quantity numeric(18,3) not null check (quantity > 0),
  unit_price numeric(18,2) not null check (unit_price >= 0),
  line_amount numeric(18,2) generated always as (round(quantity * unit_price, 2)) stored,
  quantity_delivered numeric(18,3),
  quantity_accepted numeric(18,3),
  shortage_reason text
    check (shortage_reason in ('out_of_stock', 'damaged', 'wrong_item', 'expired', 'short_packed', 'refused_by_buyer')),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('marketplace.order_lines');
create index order_lines_order_index on marketplace.order_lines (order_id);
create index order_lines_listing_index on marketplace.order_lines (listing_id);
-- Buyer reaches lines through its order (the subquery runs under the orders policies).
create policy buyer_access on marketplace.order_lines
  using (exists (select 1 from marketplace.orders
                  where orders.id = order_lines.order_id
                    and orders.buyer_organization_id = core.current_organization_id()))
  with check (exists (select 1 from marketplace.orders
                       where orders.id = order_lines.order_id
                         and orders.buyer_organization_id = core.current_organization_id()));

create table marketplace.shipments (
  id uuid primary key default core.generate_uuid_v7(),
  -- The vendor's organization.
  organization_id uuid not null references core.organizations (id) on delete cascade,
  buyer_organization_id uuid not null references core.organizations (id) on delete cascade,
  order_id uuid not null references marketplace.orders (id) on delete cascade,
  driver_name text,
  driver_phone_number text,
  vehicle_description text,
  status text not null default 'preparing'
    check (status in ('preparing', 'out_for_delivery', 'arrived', 'delivered', 'received', 'failed', 'cancelled')),
  dispatched_at timestamptz,
  estimated_arrival_at timestamptz,
  delivered_at timestamptz,
  received_by_user_id uuid references core.users (id),
  received_at timestamptz,
  -- The buyer's goods receipt created when the delivery was checked in.
  goods_receipt_id uuid references purchasing.goods_receipts (id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('marketplace.shipments');
create index shipments_order_index on marketplace.shipments (order_id);
create index shipments_buyer_open_index on marketplace.shipments (buyer_organization_id, estimated_arrival_at)
  where status in ('preparing', 'out_for_delivery', 'arrived');
-- Buyer tracks the delivery and records receipt.
create policy buyer_access on marketplace.shipments
  using (buyer_organization_id = core.current_organization_id())
  with check (buyer_organization_id = core.current_organization_id());

create table marketplace.shipment_events (
  id uuid primary key default core.generate_uuid_v7(),
  -- The vendor's organization.
  organization_id uuid not null references core.organizations (id) on delete cascade,
  shipment_id uuid not null references marketplace.shipments (id) on delete cascade,
  event_type text not null
    check (event_type in ('packed', 'dispatched', 'location_update', 'delayed', 'arrived', 'delivered', 'received', 'failed')),
  latitude numeric(9,6),
  longitude numeric(9,6),
  note text,
  occurred_at timestamptz not null default now(),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('marketplace.shipment_events');
create index shipment_events_shipment_index on marketplace.shipment_events (shipment_id, occurred_at);
-- Buyer reaches events through its shipment (the subquery runs under the shipments policies).
create policy buyer_access on marketplace.shipment_events
  using (exists (select 1 from marketplace.shipments
                  where shipments.id = shipment_events.shipment_id
                    and shipments.buyer_organization_id = core.current_organization_id()))
  with check (exists (select 1 from marketplace.shipments
                       where shipments.id = shipment_events.shipment_id
                         and shipments.buyer_organization_id = core.current_organization_id()));

-- One rating per order, written by the buyer, read by the vendor.
create table marketplace.vendor_ratings (
  id uuid primary key default core.generate_uuid_v7(),
  -- The vendor's organization.
  organization_id uuid not null references core.organizations (id) on delete cascade,
  buyer_organization_id uuid not null references core.organizations (id) on delete cascade,
  vendor_id uuid not null references marketplace.vendors (id),
  order_id uuid not null unique references marketplace.orders (id) on delete cascade,
  stars integer not null check (stars between 1 and 5),
  on_time boolean,
  filled_in_full boolean,
  packing_rating integer check (packing_rating between 1 and 5),
  comment text,
  -- Quick chips: well_packed, friendly_driver, late, short_delivery ...
  tags text[] not null default '{}',
  rated_by_user_id uuid references core.users (id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('marketplace.vendor_ratings');
create index vendor_ratings_vendor_index on marketplace.vendor_ratings (vendor_id, created_at desc);
create policy buyer_access on marketplace.vendor_ratings
  using (buyer_organization_id = core.current_organization_id())
  with check (buyer_organization_id = core.current_organization_id());
-- Restrictive policies (ANDed with the ones above): only the buyer may write, change or remove
-- a rating; the vendor can read its ratings but not edit them.
create policy only_buyer_inserts on marketplace.vendor_ratings as restrictive for insert
  with check (buyer_organization_id = core.current_organization_id());
create policy only_buyer_updates on marketplace.vendor_ratings as restrictive for update
  using (buyer_organization_id = core.current_organization_id());
create policy only_buyer_deletes on marketplace.vendor_ratings as restrictive for delete
  using (buyer_organization_id = core.current_organization_id());
