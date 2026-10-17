//! What the generator is expected to reject and generate in each fixture.

/// What the generator is expected to reject in one fixture, and the query
/// functions it is expected to generate. Fixtures not listed here must
/// generate every table and query.
pub(super) struct Expected {
    /// The fixture's directory under `src/db`, e.g. `legacy_modules/hr`.
    pub(super) fixture: &'static str,
    /// Rejected tables, each with a fragment of the reason.
    pub(super) tables: &'static [(&'static str, &'static str)],
    /// Rejected queries, each with a fragment of the reason, in file order.
    pub(super) queries: &'static [(&'static str, &'static str)],
    /// Generated query functions, in file order.
    pub(super) functions: &'static [&'static str],
}

pub(super) const EXPECTED: &[Expected] = &[
    // The kept-table scenarios (see `KEPT`) skip `orders` on their second run.
    Expected {
        fixture: "kept/child_foreign_key",
        tables: &[("orders", "ADD COLUMN: column `client_ip`")],
        queries: &[],
        functions: &[],
    },
    Expected {
        fixture: "kept/value_lists",
        tables: &[("orders", "ADD COLUMN: column `client_ip`")],
        queries: &[],
        functions: &[],
    },
    Expected {
        fixture: "legacy_modules/accounting",
        tables: &[],
        queries: &[],
        functions: &["create_account", "list_accounts", "deactivate_account"],
    },
    Expected {
        fixture: "legacy_modules/hr",
        // `employment_type` maps to `types::EmploymentType` and `tenure
        // INTERVAL` to `PgInterval`, so `employees` and its query generate.
        tables: &[],
        queries: &[],
        functions: &["create_department", "list_employees_by_department"],
    },
    Expected {
        fixture: "legacy_modules/inventory",
        tables: &[],
        // `products.sku` is not unique in this schema, so the upsert would
        // fail at runtime.
        queries: &[(
            "UpsertProduct",
            "ON CONFLICT (sku) matches no unique key; unique keys of `products`: (id)",
        )],
        functions: &[
            "create_product",
            "create_products",
            "update_product_stock",
            "delete_product",
            "list_products_by_category",
            "list_products_offset",
        ],
    },
    Expected {
        fixture: "legacy_modules/sales",
        tables: &[],
        queries: &[("CreateOrderWithItems", "contains 4 statements")],
        functions: &[
            "create_customer",
            "list_orders_cursor",
            "update_order_status",
        ],
    },
];

/// A kept-table scenario (ADR 0001): `initial` generates strictly, then a
/// lenient run over `initial` plus `breaking` skips `orders`, whose resource
/// directory is kept from the first run. The kept files and the regenerated
/// ones must compile together.
pub(super) struct KeptScenario {
    /// The output directory under `src/db/kept`.
    pub(super) name: &'static str,
    pub(super) initial: &'static str,
    pub(super) breaking: &'static str,
}

/// A later migration `orders` cannot follow: rok-sqlc has no mapping for
/// `POINT`.
const BREAK_ORDERS: &str = "ALTER TABLE orders ADD COLUMN client_ip POINT;\n";

pub(super) const KEPT: &[KeptScenario] = &[
    // The kept `models.rs` uses the value-list enum, so `types.rs` must
    // still define it.
    KeptScenario {
        name: "value_lists",
        initial: "CREATE TABLE tags (id BIGSERIAL PRIMARY KEY, label TEXT NOT NULL);\n\
                  CREATE TABLE orders (\n\
                      id BIGSERIAL PRIMARY KEY,\n\
                      status TEXT NOT NULL CHECK (status IN ('open', 'paid'))\n\
                  );\n",
        breaking: BREAK_ORDERS,
    },
    // The kept `relationships.rs` loads `LineItem`s by `OrderId`, so the
    // regenerated child must keep the typed id, and its own loaders reach the
    // kept `Order`.
    KeptScenario {
        name: "child_foreign_key",
        initial: "CREATE TABLE orders (id BIGSERIAL PRIMARY KEY, placed_at TIMESTAMPTZ NOT NULL);\n\
                  CREATE TABLE line_items (\n\
                      id BIGSERIAL PRIMARY KEY,\n\
                      order_id BIGINT NOT NULL REFERENCES orders (id),\n\
                      quantity INTEGER NOT NULL\n\
                  );\n",
        breaking: BREAK_ORDERS,
    },
];
