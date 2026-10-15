-- Module: purchasing
-- Migration 0001: suppliers, what they supply, purchase orders, goods receipts and supplier bills.

create schema purchasing;

create table purchasing.suppliers (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  name text not null,
  contact_person_name text,
  phone_number text,
  email text,
  address text,
  tax_identification_number text,
  vat_registration_number text,
  payment_terms_days integer not null default 0 check (payment_terms_days >= 0),
  -- Usual days between ordering and delivery, used for reorder suggestions.
  lead_time_days integer check (lead_time_days >= 0),
  -- True when this supplier is also a vendor on the rok marketplace.
  is_marketplace_vendor boolean not null default false,
  -- marketplace.vendors id; no foreign key because marketplace depends on purchasing, not the reverse.
  marketplace_vendor_id uuid,
  notes text,
  is_active boolean not null default true,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('purchasing.suppliers');
create index suppliers_name_search_index on purchasing.suppliers using gin (to_tsvector('simple', name));
create index suppliers_marketplace_vendor_index on purchasing.suppliers (marketplace_vendor_id) where marketplace_vendor_id is not null;

-- Which supplier sells which product, under what code, pack size and last price.
create table purchasing.supplier_products (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  supplier_id uuid not null references purchasing.suppliers (id) on delete cascade,
  product_id uuid not null references catalog.products (id) on delete cascade,
  product_variant_id uuid references catalog.product_variants (id) on delete cascade,
  supplier_item_code text,
  supplier_item_name text,
  -- Unit the supplier sells in (carton, crate, bag) and how many selling units it holds.
  purchase_unit_id uuid references catalog.units_of_measure (id),
  units_per_purchase_unit numeric(18,3) not null default 1 check (units_per_purchase_unit > 0),
  last_cost_price numeric(18,2),
  minimum_order_quantity numeric(18,3) not null default 1 check (minimum_order_quantity >= 0),
  is_preferred_supplier boolean not null default false,
  last_purchased_on date,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('purchasing.supplier_products');
create unique index supplier_products_unique_index on purchasing.supplier_products (
  supplier_id, product_id,
  coalesce(product_variant_id, '00000000-0000-0000-0000-000000000000'::uuid)
) where deleted_at is null;
create index supplier_products_product_index on purchasing.supplier_products (product_id);

create table purchasing.purchase_orders (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  purchase_order_number text not null,
  supplier_id uuid not null references purchasing.suppliers (id),
  branch_id uuid references core.branches (id),
  -- Where the goods will be received.
  receiving_stock_location_id uuid references inventory.stock_locations (id),
  status text not null default 'draft'
    check (status in ('draft', 'waiting_for_approval', 'approved', 'sent', 'partly_received', 'received', 'cancelled')),
  ordered_on date,
  expected_on date,
  currency_code text not null default 'TZS',
  subtotal_amount numeric(18,2) not null default 0,
  discount_amount numeric(18,2) not null default 0 check (discount_amount >= 0),
  tax_amount numeric(18,2) not null default 0,
  total_amount numeric(18,2) not null default 0,
  created_by_user_id uuid references core.users (id),
  approved_by_user_id uuid references core.users (id),
  approved_at timestamptz,
  sent_at timestamptz,
  notes text,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, purchase_order_number)
);
select core.prepare_table('purchasing.purchase_orders');
create index purchase_orders_supplier_index on purchasing.purchase_orders (supplier_id, created_at desc);
create index purchase_orders_status_index on purchasing.purchase_orders (organization_id, status);

create table purchasing.purchase_order_lines (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  purchase_order_id uuid not null references purchasing.purchase_orders (id) on delete cascade,
  line_number integer not null,
  product_id uuid not null references catalog.products (id),
  product_variant_id uuid references catalog.product_variants (id),
  description text,
  quantity_ordered numeric(18,3) not null check (quantity_ordered > 0),
  -- Running total from goods receipts, to show what is still outstanding.
  quantity_received numeric(18,3) not null default 0 check (quantity_received >= 0),
  unit_cost numeric(18,2) not null default 0,
  discount_amount numeric(18,2) not null default 0 check (discount_amount >= 0),
  tax_rate_percent numeric(6,3) not null default 0,
  tax_amount numeric(18,2) not null default 0,
  line_total_amount numeric(18,2) not null default 0,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (purchase_order_id, line_number)
);
select core.prepare_table('purchasing.purchase_order_lines');
create index purchase_order_lines_product_index on purchasing.purchase_order_lines (product_id);

-- A delivery checked in at the back door, with or without a purchase order.
create table purchasing.goods_receipts (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  goods_receipt_number text not null,
  purchase_order_id uuid references purchasing.purchase_orders (id),
  supplier_id uuid not null references purchasing.suppliers (id),
  stock_location_id uuid not null references inventory.stock_locations (id),
  -- Posting adds the received quantities to stock as purchase_receipt movements.
  status text not null default 'draft' check (status in ('draft', 'posted', 'cancelled')),
  -- Number printed on the supplier's delivery note.
  supplier_delivery_note_number text,
  received_by_user_id uuid references core.users (id),
  received_at timestamptz not null default now(),
  posted_at timestamptz,
  notes text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  unique (organization_id, goods_receipt_number)
);
select core.prepare_table('purchasing.goods_receipts');
create index goods_receipts_purchase_order_index on purchasing.goods_receipts (purchase_order_id);
create index goods_receipts_supplier_index on purchasing.goods_receipts (supplier_id, received_at desc);

create table purchasing.goods_receipt_lines (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  goods_receipt_id uuid not null references purchasing.goods_receipts (id) on delete cascade,
  purchase_order_line_id uuid references purchasing.purchase_order_lines (id),
  product_id uuid not null references catalog.products (id),
  product_variant_id uuid references catalog.product_variants (id),
  quantity_received numeric(18,3) not null default 0 check (quantity_received >= 0),
  -- Damaged, expired or wrong items refused at delivery.
  quantity_rejected numeric(18,3) not null default 0 check (quantity_rejected >= 0),
  rejection_reason text,
  unit_cost numeric(18,2) not null default 0,
  batch_number text,
  expires_on date,
  -- Batch created in inventory when the receipt is posted.
  stock_batch_id uuid references inventory.stock_batches (id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (quantity_rejected = 0 or rejection_reason is not null)
);
select core.prepare_table('purchasing.goods_receipt_lines');
create index goods_receipt_lines_receipt_index on purchasing.goods_receipt_lines (goods_receipt_id);
create index goods_receipt_lines_order_line_index on purchasing.goods_receipt_lines (purchase_order_line_id) where purchase_order_line_id is not null;

-- The supplier's invoice to the business, tracked until paid.
create table purchasing.supplier_bills (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  supplier_id uuid not null references purchasing.suppliers (id),
  purchase_order_id uuid references purchasing.purchase_orders (id),
  goods_receipt_id uuid references purchasing.goods_receipts (id),
  -- Number on the supplier's invoice.
  bill_number text not null,
  billed_on date not null default current_date,
  due_on date,
  tax_amount numeric(18,2) not null default 0,
  amount numeric(18,2) not null check (amount >= 0),
  paid_amount numeric(18,2) not null default 0 check (paid_amount >= 0),
  status text not null default 'unpaid' check (status in ('draft', 'unpaid', 'partly_paid', 'paid', 'cancelled')),
  attachment_id uuid references core.attachments (id),
  notes text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('purchasing.supplier_bills');
create unique index supplier_bills_number_unique_index on purchasing.supplier_bills (organization_id, supplier_id, bill_number) where deleted_at is null;
create index supplier_bills_due_index on purchasing.supplier_bills (organization_id, due_on) where status in ('unpaid', 'partly_paid');
