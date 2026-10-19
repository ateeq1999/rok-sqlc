// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Mail, with no business meaning: a [`Mail`] message, the [`Mailer`] port
//! its transports implement ([`transport`]), the messages the account flows
//! send ([`Templates`]), and the outbox that makes sending part of a flow's
//! transaction ([`Outbox`], [`Sender`]).
//!
//! A flow never sends: it renders a message, enqueues it on its own
//! transaction and commits through the outbox, which wakes the sender. The
//! sender task (started by `app::run`) delivers it, retries a failure with
//! backoff, and marks a row `dead` after [`outbox::MAX_ATTEMPTS`].

pub mod outbox;
pub mod templates;
pub mod transport;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub use outbox::{Delivered, Outbox, Sender};
pub use templates::Templates;
pub use transport::{LogMailer, MemoryMailer, SmtpMailer};

/// A rendered mail: one recipient, a subject, and a text and an HTML body.
/// The outbox stores it as JSON until it is sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mail {
    /// The recipient's address.
    pub to: String,
    /// The subject line.
    pub subject: String,
    /// The plain-text body.
    pub text: String,
    /// The HTML body.
    pub html: String,
}

/// Why a mail was not sent. It goes to the log and to the outbox row's
/// `last_error`, never to an API client.
#[derive(Debug, thiserror::Error)]
pub enum MailError {
    /// A mail setting (`SMTP_URL`, `MAIL_FROM`) is invalid.
    #[error("invalid mail setting: {0}")]
    Config(String),
    /// The message can't be built, such as an invalid recipient.
    #[error("could not build the message: {0}")]
    Build(String),
    /// The transport failed to deliver it.
    #[error("{0}")]
    Delivery(String),
}

/// The port every transport implements: send one mail, now.
#[async_trait]
pub trait Mailer: Send + Sync {
    /// Sends `mail`, or says why it was not sent.
    async fn send(&self, mail: &Mail) -> Result<(), MailError>;
}
