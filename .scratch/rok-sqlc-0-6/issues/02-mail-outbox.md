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

**Status:** ready-for-agent

- [ ] Integration (PostgreSQL):
  - an enqueued row is sent and deleted;
  - a failing transport backs off, then marks the row `dead`;
  - two concurrent senders never send a row twice;
  - a rolled-back transaction sends nothing.
- [ ] Gates: clippy-clean on 1.94, `check-arch` clean, contract passes.
