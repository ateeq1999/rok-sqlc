//! The README's conventions example: the `rok` preset (see `rok-sqlc.toml`)
//! over `migrations/`.

#![deny(missing_docs)]

pub mod db;

use db::UpdateOutcome;
use db::invoices::models::{NewInvoice, UpdateInvoice};
use db::organizations::models::{NewOrganization, OrganizationId};
use db::types::InvoiceStatus;
use rust_decimal::Decimal;
use sqlx::PgPool;

/// Creates an organization, then issues, pays, and deletes an invoice in it.
///
/// # Errors
///
/// Any database error.
pub async fn invoices(pool: &PgPool, user: uuid::Uuid) -> Result<(), sqlx::Error> {
    let org = db::organizations::write::create(
        pool,
        NewOrganization {
            id: None,
            name: "Acme".into(),
        },
    )
    .await?;
    pay_first_invoice(pool, org.id, user).await
}

async fn pay_first_invoice(
    pool: &PgPool,
    org: OrganizationId,
    user: uuid::Uuid,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    // For row-level security policies and audit triggers.
    db::session::set_tenant(&mut tx, org).await?;
    db::session::set_user(&mut tx, user).await?;

    // The tenant comes right after the executor, on every function.
    let invoice = db::invoices::write::create(
        &mut *tx,
        org,
        NewInvoice {
            id: None,
            number: "INV-001".into(),
            total: Decimal::new(25_000, 2),
            status: None,
        },
    )
    .await?;

    // Optimistic concurrency: the update applies only at the version read.
    let patch = UpdateInvoice {
        status: Some(InvoiceStatus::Paid),
        ..UpdateInvoice::default()
    };
    let outcome =
        db::invoices::write::update(&mut *tx, org, invoice.id, invoice.row_version, patch).await?;
    match outcome {
        UpdateOutcome::Updated(paid) => assert_eq!(paid.row_version, invoice.row_version + 1),
        UpdateOutcome::NotFound => println!("deleted meanwhile"),
        UpdateOutcome::VersionConflict => println!("changed meanwhile; reload and retry"),
    }

    // Soft delete: reads skip the row until it is restored.
    db::invoices::write::delete(&mut *tx, org, invoice.id).await?;
    let hidden = db::invoices::read::get_by_number(&mut *tx, org, "INV-001").await?;
    assert!(hidden.is_none());
    db::invoices::write::restore(&mut *tx, org, invoice.id).await?;
    tx.commit().await
}
