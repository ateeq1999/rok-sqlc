-- Module: marketplace
-- Migration 0001: vendors, their verification documents, delivery areas and listings.
--
-- Vendors are businesses on the platform. On vendor-owned tables organization_id is the
-- VENDOR's organization, so the standard organization_isolation policy gives the vendor full access.
-- Extra marketplace_browse policies (select only) let every other business see verified vendors
-- and visible listings. PostgreSQL ORs permissive policies together.

create schema marketplace;

create table marketplace.vendors (
  id uuid primary key default core.generate_uuid_v7(),
  -- The vendor's own organization.
  organization_id uuid not null references core.organizations (id) on delete cascade,
  display_name text not null,
  description text,
  logo_attachment_id uuid references core.attachments (id),
  contact_phone_number text,
  verification_status text not null default 'applied'
    check (verification_status in ('applied', 'documents_uploaded', 'under_review', 'verified', 'suspended', 'rejected')),
  verified_at timestamptz,
  minimum_order_amount numeric(18,2) not null default 0,
  free_delivery_over_amount numeric(18,2),
  -- Orders placed after this local time go out the next delivery day.
  order_cutoff_time time,
  accepts_trade_credit boolean not null default true,
  accepts_cash_on_delivery boolean not null default true,
  -- Platform commission taken from each order.
  commission_percent numeric(6,3) not null default 0,
  payout_method text check (payout_method in ('bank', 'mobile_money')),
  -- Masked account or wallet number; full details live with the payment provider.
  payout_account_reference text,
  rating_average numeric(3,2) check (rating_average between 0 and 5),
  rating_count integer not null default 0,
  on_time_percent numeric(6,3),
  filled_in_full_percent numeric(6,3),
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1,
  check (verification_status <> 'verified' or verified_at is not null)
);
select core.prepare_table('marketplace.vendors');
-- One vendor profile per organization.
create unique index vendors_organization_unique_index on marketplace.vendors (organization_id) where deleted_at is null;
create index vendors_verified_index on marketplace.vendors (display_name) where verification_status = 'verified' and deleted_at is null;
-- Every business may read verified vendors.
create policy marketplace_browse on marketplace.vendors for select
  using (verification_status = 'verified' and deleted_at is null);

-- Onboarding paperwork, reviewed by platform staff. Vendor-only (standard isolation).
create table marketplace.vendor_documents (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  vendor_id uuid not null references marketplace.vendors (id) on delete cascade,
  document_type text not null
    check (document_type in ('business_registration', 'tax_identification_certificate', 'trading_licence', 'warehouse_photo')),
  attachment_id uuid not null references core.attachments (id),
  status text not null default 'uploaded' check (status in ('uploaded', 'approved', 'rejected', 'expired')),
  expires_on date,
  review_notes text,
  reviewed_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('marketplace.vendor_documents');
create index vendor_documents_vendor_index on marketplace.vendor_documents (vendor_id, document_type);

create table marketplace.vendor_delivery_areas (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  vendor_id uuid not null references marketplace.vendors (id) on delete cascade,
  region text not null,
  district text not null,
  delivery_fee_amount numeric(18,2) not null default 0,
  -- ISO weekdays the vendor delivers there: 1 = Monday ... 7 = Sunday.
  delivery_days integer[] not null default '{1,2,3,4,5,6}' check (delivery_days <@ '{1,2,3,4,5,6,7}'::integer[]),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('marketplace.vendor_delivery_areas');
create unique index vendor_delivery_areas_unique_index on marketplace.vendor_delivery_areas (vendor_id, lower(region), lower(district)) where deleted_at is null;
create index vendor_delivery_areas_lookup_index on marketplace.vendor_delivery_areas (lower(region), lower(district)) where deleted_at is null;
-- Every business may read the delivery areas of verified vendors.
create policy marketplace_browse on marketplace.vendor_delivery_areas for select
  using (deleted_at is null and exists (
    select 1 from marketplace.vendors
     where vendors.id = vendor_delivery_areas.vendor_id
       and vendors.verification_status = 'verified' and vendors.deleted_at is null));

create table marketplace.listings (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  vendor_id uuid not null references marketplace.vendors (id) on delete cascade,
  -- The vendor's own product this listing sells, when the vendor also runs rok POS stock.
  catalog_product_id uuid references catalog.products (id),
  title text not null,
  description text,
  -- What one unit is: "25 kg bag", "crate of 24 x 500 ml".
  unit_description text not null,
  browse_category text,
  barcode text,
  image_attachment_id uuid references core.attachments (id),
  minimum_order_quantity numeric(18,3) not null default 1 check (minimum_order_quantity > 0),
  stock_available numeric(18,3),
  is_visible boolean not null default true,
  custom_fields jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('marketplace.listings');
create index listings_vendor_index on marketplace.listings (vendor_id);
create index listings_catalog_product_index on marketplace.listings (catalog_product_id) where catalog_product_id is not null;
create index listings_barcode_index on marketplace.listings (barcode) where barcode is not null;
create index listings_title_search_index on marketplace.listings using gin (to_tsvector('simple', title));
-- Every business may read visible listings of verified vendors.
create policy marketplace_browse on marketplace.listings for select
  using (is_visible and deleted_at is null and exists (
    select 1 from marketplace.vendors
     where vendors.id = listings.vendor_id
       and vendors.verification_status = 'verified' and vendors.deleted_at is null));

-- Quantity prices: 1–9 bags at 18,500, 10+ at 17,900.
create table marketplace.listing_price_tiers (
  id uuid primary key default core.generate_uuid_v7(),
  organization_id uuid not null references core.organizations (id) on delete cascade,
  listing_id uuid not null references marketplace.listings (id) on delete cascade,
  minimum_quantity numeric(18,3) not null default 1 check (minimum_quantity > 0),
  unit_price numeric(18,2) not null check (unit_price >= 0),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  row_version bigint not null default 1
);
select core.prepare_table('marketplace.listing_price_tiers');
create unique index listing_price_tiers_unique_index on marketplace.listing_price_tiers (listing_id, minimum_quantity) where deleted_at is null;
-- Readable wherever the listing itself is readable (the subquery runs under the listings policies).
create policy marketplace_browse on marketplace.listing_price_tiers for select
  using (deleted_at is null and exists (
    select 1 from marketplace.listings where listings.id = listing_price_tiers.listing_id));
