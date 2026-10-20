//! The mail outbox: a flow enqueues a mail on its transaction, and the
//! sender delivers it once the transaction commits, retrying a failing
//! transport with backoff until the row is `dead`.

mod support;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum_api::db::types::OutboundMailStatus;
use axum_api::shared::mail::{Mail, MailError, Mailer, MemoryMailer, Outbox, Sender, Templates};
use support::{APP_URL, Database, TestResult, mailed, mailed_token};

/// A mail to `to`.
fn mail(to: &str) -> Mail {
    Mail {
        to: to.to_owned(),
        subject: "Hello".to_owned(),
        text: "Hello there".to_owned(),
        html: "<p>Hello there</p>".to_owned(),
    }
}

/// A transport that always fails.
struct Failing;

#[async_trait]
impl Mailer for Failing {
    async fn send(&self, _mail: &Mail) -> Result<(), MailError> {
        Err(MailError::Delivery("the relay said no".to_owned()))
    }
}

/// The outbox's rows: their status, attempts and last error.
async fn rows(
    pool: &sqlx::PgPool,
) -> Result<Vec<(OutboundMailStatus, i32, Option<String>)>, sqlx::Error> {
    sqlx::query_as("SELECT status, attempts, last_error FROM outbound_mail ORDER BY id")
        .fetch_all(pool)
        .await
}

/// Enqueues `count` mails, one transaction each.
async fn enqueue(pool: &sqlx::PgPool, outbox: &Outbox, count: usize) -> TestResult {
    for n in 0..count {
        let mut tx = pool.begin().await?;
        outbox
            .enqueue(&mut tx, &mail(&format!("user{n}@test.invalid")))
            .await?;
        outbox.commit(tx).await?;
    }
    Ok(())
}

#[tokio::test]
async fn an_enqueued_mail_is_sent_then_deleted() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let outbox = Outbox::new();
    enqueue(&db.pool, &outbox, 1).await?;
    let mailer = Arc::new(MemoryMailer::default());
    let sender = Sender::new(db.pool.clone(), mailer.clone(), &outbox);

    let delivered = sender.deliver_due().await?;
    assert_eq!((delivered.sent, delivered.failed), (1, 0));
    assert_eq!(mailer.sent(), vec![mail("user0@test.invalid")]);
    assert!(rows(&db.pool).await?.is_empty());
    db.drop().await
}

#[tokio::test]
async fn a_failing_transport_backs_off_then_the_row_is_dead() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let outbox = Outbox::new();
    enqueue(&db.pool, &outbox, 1).await?;

    // The default backoff: after one failure the row waits.
    let sender = Sender::new(db.pool.clone(), Arc::new(Failing), &outbox);
    let delivered = sender.deliver_due().await?;
    assert_eq!((delivered.sent, delivered.failed), (0, 1));
    let error = Some("the relay said no".to_owned());
    assert_eq!(
        rows(&db.pool).await?,
        vec![(OutboundMailStatus::Pending, 1, error.clone())]
    );
    let delivered = sender.deliver_due().await?;
    assert_eq!((delivered.sent, delivered.failed), (0, 0), "not due yet");
    let waits: bool = sqlx::query_scalar("SELECT next_attempt_at > now() FROM outbound_mail")
        .fetch_one(&db.pool)
        .await?;
    assert!(waits);

    // Once the wait is over, with no backoff, four more failures make five: the row is dead and
    // keeps its error, and it is never tried again.
    sqlx::query("UPDATE outbound_mail SET next_attempt_at = now()")
        .execute(&db.pool)
        .await?;
    let sender =
        Sender::new(db.pool.clone(), Arc::new(Failing), &outbox).with_backoff(Duration::ZERO);
    for _ in 0..4 {
        assert_eq!(sender.deliver_due().await?.failed, 1);
    }
    assert_eq!(
        rows(&db.pool).await?,
        vec![(OutboundMailStatus::Dead, 5, error)]
    );
    let mailer = Arc::new(MemoryMailer::default());
    let sender = Sender::new(db.pool.clone(), mailer.clone(), &outbox).with_backoff(Duration::ZERO);
    assert_eq!(sender.deliver_due().await?.sent, 0);
    assert!(mailer.sent().is_empty());
    db.drop().await
}

#[tokio::test]
async fn a_dead_row_keeps_its_address_and_subject_but_not_its_link() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let outbox = Outbox::new();
    let link = Templates::new(APP_URL).password_reset("ada@test.invalid", "the-raw-token");
    let mut tx = db.pool.begin().await?;
    outbox.enqueue(&mut tx, &link).await?;
    outbox.commit(tx).await?;
    let payload = || async {
        sqlx::query_scalar::<_, serde_json::Value>("SELECT payload FROM outbound_mail")
            .fetch_one(&db.pool)
            .await
    };

    // A pending row must still carry the link it is to send.
    let sender =
        Sender::new(db.pool.clone(), Arc::new(Failing), &outbox).with_backoff(Duration::ZERO);
    sender.deliver_due().await?;
    assert!(payload().await?.to_string().contains("the-raw-token"));

    for _ in 0..4 {
        sender.deliver_due().await?;
    }
    assert_eq!(rows(&db.pool).await?[0].0, OutboundMailStatus::Dead);
    let dead = payload().await?;
    assert!(!dead.to_string().contains("the-raw-token"), "{dead}");
    assert_eq!(dead["to"], "ada@test.invalid");
    assert_eq!(dead["subject"], link.subject.as_str());
    db.drop().await
}

/// A transport that takes its time, so two senders overlap.
struct Slow(MemoryMailer);

#[async_trait]
impl Mailer for Slow {
    async fn send(&self, mail: &Mail) -> Result<(), MailError> {
        tokio::time::sleep(Duration::from_millis(5)).await;
        self.0.send(mail).await
    }
}

#[tokio::test]
async fn two_concurrent_senders_never_send_a_row_twice() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let outbox = Outbox::new();
    enqueue(&db.pool, &outbox, 40).await?;
    let first = Arc::new(Slow(MemoryMailer::default()));
    let second = Arc::new(Slow(MemoryMailer::default()));
    let drain = |sender: Sender| async move {
        let mut sent = 0;
        loop {
            let delivered = sender.deliver_due().await?;
            if delivered.sent == 0 {
                return Ok::<usize, axum_api::error::AppError>(sent);
            }
            sent += delivered.sent;
        }
    };
    let (a, b) = tokio::join!(
        drain(Sender::new(db.pool.clone(), first.clone(), &outbox)),
        drain(Sender::new(db.pool.clone(), second.clone(), &outbox)),
    );
    let (a, b) = (a?, b?);
    assert!(a > 0 && b > 0, "both senders sent: {a} and {b}");

    let mut recipients: Vec<String> = first
        .0
        .sent()
        .into_iter()
        .chain(second.0.sent())
        .map(|mail| mail.to)
        .collect();
    assert_eq!(recipients.len(), 40);
    recipients.sort();
    recipients.dedup();
    assert_eq!(recipients.len(), 40, "a row was sent twice");
    assert!(rows(&db.pool).await?.is_empty());
    db.drop().await
}

#[tokio::test]
async fn a_rolled_back_transaction_sends_nothing() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let outbox = Outbox::new();
    let mut tx = db.pool.begin().await?;
    outbox
        .enqueue(&mut tx, &mail("nobody@test.invalid"))
        .await?;
    tx.rollback().await?;

    let mailer = Arc::new(MemoryMailer::default());
    let sender = Sender::new(db.pool.clone(), mailer.clone(), &outbox);
    assert_eq!(sender.deliver_due().await?.sent, 0);
    assert!(mailer.sent().is_empty());
    assert!(rows(&db.pool).await?.is_empty());
    db.drop().await
}

#[tokio::test]
async fn the_running_sender_wakes_on_commit_and_stops_on_shutdown() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let outbox = Outbox::new();
    let mailer = Arc::new(MemoryMailer::default());
    let sender = Sender::new(db.pool.clone(), mailer.clone(), &outbox);
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(sender.run(async {
        let _ = stopped.await;
    }));
    // Let the sender reach its wait first: it must be woken, not find the
    // row on its first pass.
    tokio::time::sleep(Duration::from_millis(100)).await;

    enqueue(&db.pool, &outbox, 1).await?;
    // Well before the 5-second tick.
    let mut waited = Duration::ZERO;
    while mailer.sent().is_empty() && waited < Duration::from_secs(2) {
        tokio::time::sleep(Duration::from_millis(20)).await;
        waited += Duration::from_millis(20);
    }
    assert_eq!(mailer.sent(), vec![mail("user0@test.invalid")]);

    let _ = stop.send(());
    tokio::time::timeout(Duration::from_secs(2), task).await??;
    db.drop().await
}

#[tokio::test]
async fn a_test_reads_the_token_a_delivered_link_carries() -> TestResult {
    let Some(db) = Database::fresh().await? else {
        return Ok(());
    };
    let templates = Templates::new(APP_URL);
    let outbox = Outbox::new();
    let mut tx = db.pool.begin().await?;
    let mail = templates.password_reset("ana@test.invalid", "raw-Token_1");
    outbox.enqueue(&mut tx, &mail).await?;
    outbox.commit(tx).await?;

    let sent = mailed(&db.pool).await?;
    assert_eq!(sent, vec![mail]);
    assert_eq!(mailed_token(&sent[0]).as_deref(), Some("raw-Token_1"));
    assert!(
        sent[0]
            .text
            .contains("http://app.test/reset-password?token=raw-Token_1")
    );
    db.drop().await
}
