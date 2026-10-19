# 02: Mail: the `Mailer` port, transports, outbox and sender

**What to build:** decisions Q6 and Q24.

- **`shared/mail/`:**
  - A `Mailer` port (`#[async_trait]`, `Arc<dyn Mailer>`) with transports `smtp` (`lettre`, rustls, from `SMTP_URL`/`MAIL_FROM`), `log` (logs the message, the default), and a test-only in-memory one that captures sent messages so tests can read the tokens.
  - A small message type (to, subject, text, html).
  - Templates for verification, password reset, email change (new address), email-change notice (old address), password-changed notice and invitation. Links are built from `APP_URL`.
- **Outbox:** `enqueue(&mut tx, message)` writes an `outbound_mail` row on the flow's transaction. Its queries live in `queries/mail.sql`.
- **Sender task, started from `main.rs`/`app.rs`:**
  - It wakes every 5 seconds or when notified (a `tokio::sync::Notify` signalled after a flow commits).
  - It claims pending, due rows with `FOR UPDATE SKIP LOCKED` in small batches, sends them, and deletes each on success.
  - On failure it increments `attempts` and sets `next_attempt_at` with exponential backoff. After 5 attempts the row becomes `dead`, keeping `last_error`.
  - It stops on server shutdown. `/health/ready` ignores mail.

**Blocked by:** 01

**Status:** resolved

- [x] Integration (PostgreSQL):
  - an enqueued row is sent and deleted;
  - a failing transport backs off, then marks the row `dead`;
  - two concurrent senders never send a row twice;
  - a rolled-back transaction sends nothing.
- [x] Gates: clippy-clean on 1.94, `check-arch` clean, contract passes.

## Comments

**Implemented on `rok-sqlc-0-6-02`.** What tickets 03–05 build on:

- **Module `crate::shared::mail`** (`src/shared/mail/{mod,transport,templates,outbox}.rs`): `Mail { to, subject, text, html }` (serde; stored as the row's JSON payload), `MailError::{Config, Build, Delivery}` (never reaches a client), and the port `#[async_trait] trait Mailer: Send + Sync { async fn send(&self, &Mail) -> Result<(), MailError> }`. Transports: `SmtpMailer::new(SMTP_URL, MAIL_FROM)` (lettre 0.11, rustls/ring, no connection pool), `LogMailer` (to/subject at `info`, body at `debug`), `MemoryMailer` (`.sent() -> Vec<Mail>`). `transport::from_config(&Config) -> Arc<dyn Mailer>`.
- **Wiring changed:** `app::configure(cfg, &AppState)`. `AppState { pool, keys: web::Data<JwtKeys>, outbox: web::Data<Outbox>, templates: web::Data<Templates> }` (pub fields, `Clone`; `AppState::new(pool, &Config)`). All four are registered as app data, so a handler takes `web::Data<Outbox>` / `web::Data<Templates>`. Add new state (auth settings, rate limit) as fields here and in `tests/support::state`. `run` starts the `Sender` with `actix_web::rt::spawn` before serving and stops it (oneshot) after the server returns. `/health/ready` ignores mail.
- **Enqueue from a flow** (service code never names sqlx types):
  ```rust
  let mut tx = pool.begin().await?;
  // ... write the user / auth_tokens row on &mut *tx ...
  outbox.enqueue(&mut tx, &templates.verification(&email, &token.raw)).await?;
  outbox.commit(tx).await?; // commits, then wakes the sender
  ```
  `enqueue(&self, &mut PgConnection, &Mail) -> Result<(), AppError>`; `commit(&self, Transaction) -> Result<(), AppError>`; or commit yourself and call `outbox.wake()`. A rolled-back transaction sends nothing.
- **Templates** (`Templates::new(app_url)`): `verification(to, token)` → `{APP_URL}/verify-email?token=`, `password_reset(to, token)` → `/reset-password`, `email_change(new_address, token)` → `/confirm-email-change`, `email_change_notice(old_address, new_email)`, `password_changed(to)`, `invitation(to, token)` → `/accept-invitation`. Path constants `VERIFY_EMAIL_PATH` etc. and `link(path, token)` are public. To add one: a method calling the private `compose(to, subject, &[paragraphs], Option<Action { label, url }>)`; HTML is escaped there.
- **Sender** (`Sender::new(pool, Arc<dyn Mailer>, &Outbox)`, `.with_backoff(Duration)`, `.deliver_due() -> Result<Delivered { sent, failed }, AppError>`, `.run(shutdown)`): one transaction per batch of 10, claimed `FOR UPDATE SKIP LOCKED` and held while sending; sent rows deleted; a failure sets `attempts`, `last_error`, `next_attempt_at = now() + 30s·2^(n-1)` (database clock); `dead` at `MAX_ATTEMPTS = 5`. Wakes on `Outbox` notify or every 5 s. At-least-once delivery.
- **Queries:** `queries/mail.sql` → `db::queries::mail::{enqueue_mail, claim_due_mail, delete_sent_mail, record_failed_mail}` (no `mail` feature, so anyone may call it; only `shared/mail` does).
- **Tests:** `tests/support` adds `APP_URL = "http://app.test"`, `state(&pool)`, `mailed(&pool) -> Vec<Mail>` (delivers every due row through a fresh `MemoryMailer` and returns what was sent, oldest first; sent rows are deleted, so a second call returns only new mail) and `mailed_token(&Mail) -> Option<String>` (the `?token=` of the text body). `call` builds the app from `state(pool)`. `tests/mail.rs` covers send+delete, backoff→dead, two concurrent senders, rollback, wake/shutdown of the running task, and token extraction.
- **Dependencies added:** `async-trait = "0.1"`, `lettre` (0.11, `default-features = false`, `builder`, `hostname`, `smtp-transport`, `tokio1-rustls-tls`), `tokio = { version = "1", features = ["macros", "sync", "time"] }`. README's block matches.
