// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The [`Mailer`] transports: [`SmtpMailer`] (`lettre` over rustls),
//! [`LogMailer`] (logs each mail; the development default) and
//! [`MemoryMailer`] (keeps each mail, for tests). [`from_config`] picks one
//! from `MAIL_TRANSPORT`.

use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use lettre::message::{Mailbox, MultiPart};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use super::{Mail, MailError, Mailer};
use crate::config::{Config, MailTransport};

/// The transport `config.mail_transport` names.
pub fn from_config(config: &Config) -> Result<Arc<dyn Mailer>, MailError> {
    match (config.mail_transport, config.smtp_url.as_deref()) {
        (MailTransport::Smtp, Some(url)) => Ok(Arc::new(SmtpMailer::new(url, &config.mail_from)?)),
        (MailTransport::Smtp, None) => Err(MailError::Config("SMTP_URL is not set".to_owned())),
        (MailTransport::Log, _) => {
            tracing::warn!("MAIL_TRANSPORT=log: mail is logged, not sent");
            Ok(Arc::new(LogMailer))
        }
    }
}

/// Sends through an SMTP relay.
pub struct SmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl SmtpMailer {
    /// A relay at `url` (`smtps://user:pass@host:465`, or `smtp://…` with
    /// `?tls=required` for STARTTLS), sending as `from`
    /// (`Name <address>`).
    pub fn new(url: &str, from: &str) -> Result<Self, MailError> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
            .map_err(|err| MailError::Config(format!("SMTP_URL: {err}")))?
            .build();
        let from = from
            .parse()
            .map_err(|err| MailError::Config(format!("MAIL_FROM: {err}")))?;
        Ok(SmtpMailer { transport, from })
    }
}

#[async_trait]
impl Mailer for SmtpMailer {
    async fn send(&self, mail: &Mail) -> Result<(), MailError> {
        let message = message(&self.from, mail)?;
        self.transport
            .send(message)
            .await
            .map(|_| ())
            .map_err(|err| MailError::Delivery(err.to_string()))
    }
}

/// `mail` from `from`, as a text and HTML alternative.
fn message(from: &Mailbox, mail: &Mail) -> Result<Message, MailError> {
    let to: Mailbox = mail
        .to
        .parse()
        .map_err(|err| MailError::Build(format!("recipient `{}`: {err}", mail.to)))?;
    Message::builder()
        .from(from.clone())
        .to(to)
        .subject(mail.subject.clone())
        .multipart(MultiPart::alternative_plain_html(
            mail.text.clone(),
            mail.html.clone(),
        ))
        .map_err(|err| MailError::Build(err.to_string()))
}

/// Logs each mail instead of sending it: the recipient and subject at
/// `info`, the text body (its links are credentials) at `debug`.
pub struct LogMailer;

#[async_trait]
impl Mailer for LogMailer {
    async fn send(&self, mail: &Mail) -> Result<(), MailError> {
        tracing::info!(to = %mail.to, subject = %mail.subject, "mail logged, not sent");
        tracing::debug!(to = %mail.to, body = %mail.text, "logged mail's body");
        Ok(())
    }
}

/// Keeps each mail it is given, so a test can read what was sent (and the
/// tokens the links carry).
#[derive(Default)]
pub struct MemoryMailer {
    sent: Mutex<Vec<Mail>>,
}

impl MemoryMailer {
    /// Every mail sent so far, oldest first.
    #[must_use]
    pub fn sent(&self) -> Vec<Mail> {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl Mailer for MemoryMailer {
    async fn send(&self, mail: &Mail) -> Result<(), MailError> {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(mail.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail(to: &str) -> Mail {
        Mail {
            to: to.to_owned(),
            subject: "Hello".to_owned(),
            text: "plain".to_owned(),
            html: "<p>html</p>".to_owned(),
        }
    }

    #[test]
    fn a_message_has_both_bodies() -> Result<(), Box<dyn std::error::Error>> {
        let from: Mailbox = "App <no-reply@example.com>".parse()?;
        let raw = String::from_utf8(message(&from, &mail("user@example.com"))?.formatted())?;
        assert!(raw.contains("multipart/alternative"));
        assert!(raw.contains("Subject: Hello"));
        assert!(raw.contains("To: user@example.com"));
        Ok(())
    }

    #[test]
    fn an_invalid_recipient_is_not_built() -> Result<(), Box<dyn std::error::Error>> {
        let from: Mailbox = "no-reply@example.com".parse()?;
        assert!(matches!(
            message(&from, &mail("not an address")),
            Err(MailError::Build(_))
        ));
        Ok(())
    }

    #[test]
    fn invalid_settings_are_reported() {
        assert!(matches!(
            SmtpMailer::new("ftp://nope", "App <a@example.com>"),
            Err(MailError::Config(_))
        ));
        assert!(matches!(
            SmtpMailer::new("smtps://mail.example.com", "not a mailbox"),
            Err(MailError::Config(_))
        ));
    }
}
