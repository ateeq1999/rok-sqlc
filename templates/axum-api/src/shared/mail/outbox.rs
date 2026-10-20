// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The mail outbox: a flow writes its mail as an `outbound_mail` row on its
//! own transaction ([`Outbox::enqueue`]), so a rolled-back flow sends
//! nothing, and commits through [`Outbox::commit`], which wakes the
//! [`Sender`]. The sender claims due rows with `FOR UPDATE SKIP LOCKED`, so
//! several instances never send a row twice, and deletes each one sent.
//!
//! Delivery is at least once: a crash after a send but before the claiming
//! transaction commits sends that mail again.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use tokio::sync::Notify;

use super::{Mail, MailError, Mailer};
use crate::db::outbound_mail::models::OutboundMailId;
use crate::db::queries::mail::{self as queries, ClaimDueMailRow, RecordFailedMailParams};
use crate::db::types::OutboundMailStatus;
use crate::error::AppError;

/// Rows one pass claims, and holds locked while it sends them.
const BATCH: i64 = 10;

/// Failed attempts after which a row is `dead`: kept with its address,
/// subject and `last_error` (its bodies are dropped), never tried again.
pub const MAX_ATTEMPTS: i32 = 5;

/// How often the sender looks for due rows when nothing wakes it.
const TICK: Duration = Duration::from_secs(5);

/// The wait after a first failure; it doubles with each further one (30s,
/// 1m, 2m, 4m).
const DEFAULT_BACKOFF: Duration = Duration::from_secs(30);

/// Where flows put their mail. Cheap to clone: every clone wakes the same
/// sender.
#[derive(Clone, Default)]
pub struct Outbox {
    wake: Arc<Notify>,
}

impl Outbox {
    /// An outbox whose [`Sender`]s are not woken yet.
    #[must_use]
    pub fn new() -> Self {
        Outbox::default()
    }

    /// Writes `mail` as a pending row on `conn`, the flow's transaction
    /// (`&mut tx`). It is sent only once that transaction commits.
    pub async fn enqueue(&self, conn: &mut PgConnection, mail: &Mail) -> Result<(), AppError> {
        let payload =
            serde_json::to_value(mail).map_err(|err| AppError::Internal(Box::new(err)))?;
        queries::enqueue_mail(conn, OutboundMailId::generate(), payload).await?;
        Ok(())
    }

    /// Commits the flow's transaction `tx`, then wakes the sender so its
    /// mail leaves now rather than at the next tick.
    pub async fn commit(&self, tx: Transaction<'_, Postgres>) -> Result<(), AppError> {
        tx.commit().await?;
        self.wake();
        Ok(())
    }

    /// Wakes the sender: call it after committing a transaction that
    /// enqueued mail without [`commit`](Self::commit).
    pub fn wake(&self) {
        self.wake.notify_one();
    }
}

/// What one [`Sender::deliver_due`] pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Delivered {
    /// Rows sent and deleted.
    pub sent: usize,
    /// Rows whose attempt failed: postponed, or `dead`.
    pub failed: usize,
}

/// Delivers the outbox's due rows through a [`Mailer`].
pub struct Sender {
    pool: PgPool,
    mailer: Arc<dyn Mailer>,
    wake: Arc<Notify>,
    backoff: Duration,
}

impl Sender {
    /// A sender of `outbox`'s rows in `pool`, through `mailer`.
    #[must_use]
    pub fn new(pool: PgPool, mailer: Arc<dyn Mailer>, outbox: &Outbox) -> Self {
        Sender {
            pool,
            mailer,
            wake: Arc::clone(&outbox.wake),
            backoff: DEFAULT_BACKOFF,
        }
    }

    /// The wait after a first failure instead of 30 seconds; it doubles
    /// with each further failure.
    #[must_use]
    pub fn with_backoff(mut self, backoff: Duration) -> Self {
        self.backoff = backoff;
        self
    }

    /// Delivers until `shutdown` completes: a pass whenever the outbox
    /// wakes it, and every 5 seconds. A pass in progress finishes first.
    pub async fn run(self, shutdown: impl Future<Output = ()>) {
        let mut shutdown = std::pin::pin!(shutdown);
        loop {
            self.deliver_all_due().await;
            tokio::select! {
                biased;
                () = &mut shutdown => return,
                () = self.wake.notified() => {}
                () = tokio::time::sleep(TICK) => {}
            }
        }
    }

    /// Passes until one claims less than a full batch. A database error is
    /// logged; the next wake or tick tries again.
    async fn deliver_all_due(&self) {
        loop {
            match self.deliver_due().await {
                Ok(delivered) if delivered.sent + delivered.failed >= BATCH as usize => {}
                Ok(_) => return,
                Err(err) => {
                    tracing::error!(error = %err, "the mail sender could not read the outbox");
                    return;
                }
            }
        }
    }

    /// One pass: claims up to a batch of pending, due rows, sends each, and
    /// deletes it or records the failure, in one transaction.
    pub async fn deliver_due(&self) -> Result<Delivered, AppError> {
        let mut tx = self.pool.begin().await?;
        let rows = queries::claim_due_mail(&mut *tx, BATCH).await?;
        let mut delivered = Delivered::default();
        for row in rows {
            match self.send(&row).await {
                Ok(()) => {
                    queries::delete_sent_mail(&mut *tx, row.id).await?;
                    delivered.sent += 1;
                }
                Err(err) => {
                    self.record_failure(&mut tx, &row, &err).await?;
                    delivered.failed += 1;
                }
            }
        }
        tx.commit().await?;
        Ok(delivered)
    }

    /// Sends the mail `row` holds.
    async fn send(&self, row: &ClaimDueMailRow) -> Result<(), MailError> {
        let mail: Mail = serde_json::from_value(row.payload.clone())
            .map_err(|err| MailError::Build(format!("unreadable payload: {err}")))?;
        self.mailer.send(&mail).await
    }

    /// Counts the failed attempt on `row`: a retry after the backoff, or
    /// `dead` after [`MAX_ATTEMPTS`], when its bodies (and the link they
    /// may carry) are dropped.
    async fn record_failure(
        &self,
        conn: &mut PgConnection,
        row: &ClaimDueMailRow,
        err: &MailError,
    ) -> Result<(), AppError> {
        let attempts = row.attempts + 1;
        let status = if attempts >= MAX_ATTEMPTS {
            tracing::error!(id = %row.id, attempts, error = %err, "mail dead after its last attempt");
            OutboundMailStatus::Dead
        } else {
            tracing::warn!(id = %row.id, attempts, error = %err, "mail not sent; will retry");
            OutboundMailStatus::Pending
        };
        let retry_in = self.backoff * 2u32.pow(u32::try_from(attempts - 1).unwrap_or(0));
        queries::record_failed_mail(
            &mut *conn,
            RecordFailedMailParams {
                id: row.id,
                attempts,
                last_error: Some(err.to_string()),
                status,
                retry_in_secs: retry_in.as_secs_f64(),
            },
        )
        .await?;
        if status == OutboundMailStatus::Dead {
            queries::redact_mail(conn, row.id).await?;
        }
        Ok(())
    }
}
