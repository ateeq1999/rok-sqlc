// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! The account flows' messages, each a [`Mail`] with a text and an HTML
//! body. A link is `{APP_URL}{path}?token={token}`: the front end at
//! `APP_URL` serves the page and posts the token back to the API.
//!
//! To add a message, add a method that calls [`Templates::compose`] with
//! its subject, paragraphs and (optional) link.

use super::Mail;

/// The page that confirms an email address.
pub const VERIFY_EMAIL_PATH: &str = "/verify-email";
/// The page that sets a new password after a reset request.
pub const RESET_PASSWORD_PATH: &str = "/reset-password";
/// The page that confirms an account's new email address.
pub const CONFIRM_EMAIL_CHANGE_PATH: &str = "/confirm-email-change";
/// The page where an invited user sets their password.
pub const ACCEPT_INVITATION_PATH: &str = "/accept-invitation";

/// A call to action: the button's label and its link.
struct Action<'a> {
    label: &'a str,
    url: String,
}

/// Renders the messages, with links under the application's URL.
#[derive(Debug, Clone)]
pub struct Templates {
    app_url: String,
}

impl Templates {
    /// Messages whose links start with `app_url` (`APP_URL`); a trailing
    /// `/` is dropped.
    #[must_use]
    pub fn new(app_url: &str) -> Self {
        Templates {
            app_url: app_url.trim_end_matches('/').to_owned(),
        }
    }

    /// `{app_url}{path}?token={token}`. A token is base64url, so it needs
    /// no escaping.
    #[must_use]
    pub fn link(&self, path: &str, token: &str) -> String {
        format!("{}{path}?token={token}", self.app_url)
    }

    /// Asks `to` to confirm their address (valid 24 hours).
    #[must_use]
    pub fn verification(&self, to: &str, token: &str) -> Mail {
        compose(
            to,
            "Verify your email address",
            &[
                "Confirm that this is your email address to activate your account. The link is valid for 24 hours.",
                "If you didn't create an account, ignore this email.",
            ],
            Some(Action {
                label: "Verify email",
                url: self.link(VERIFY_EMAIL_PATH, token),
            }),
        )
    }

    /// Lets `to` choose a new password (valid 1 hour).
    #[must_use]
    pub fn password_reset(&self, to: &str, token: &str) -> Mail {
        compose(
            to,
            "Reset your password",
            &[
                "Someone asked to reset the password of your account. Choose a new one with the link below; it is valid for 1 hour.",
                "If it wasn't you, ignore this email: your password stays the same.",
            ],
            Some(Action {
                label: "Reset password",
                url: self.link(RESET_PASSWORD_PATH, token),
            }),
        )
    }

    /// Asks the new address `to` to confirm an email change (valid 1 hour).
    #[must_use]
    pub fn email_change(&self, to: &str, token: &str) -> Mail {
        let request = format!(
            "Someone asked to change the email of an account to {to}. Confirm it with the link below; it is valid for 1 hour."
        );
        compose(
            to,
            "Confirm your new email address",
            &[
                &request,
                "If it wasn't you, ignore this email: nothing changes.",
            ],
            Some(Action {
                label: "Confirm email",
                url: self.link(CONFIRM_EMAIL_CHANGE_PATH, token),
            }),
        )
    }

    /// Tells the old address `to` that a change to `new_email` was asked.
    #[must_use]
    pub fn email_change_notice(&self, to: &str, new_email: &str) -> Mail {
        let notice = format!(
            "Someone asked to change your account's email to {new_email}. It changes once that address is confirmed."
        );
        compose(
            to,
            "Your account's email is changing",
            &[
                &notice,
                "If it wasn't you, reset your password now: nothing changes until the new address is confirmed.",
            ],
            None,
        )
    }

    /// Tells `to` that their password was changed.
    #[must_use]
    pub fn password_changed(&self, to: &str) -> Mail {
        compose(
            to,
            "Your password was changed",
            &[
                "The password of your account was just changed, and every session was signed out.",
                "If it wasn't you, reset your password now.",
            ],
            None,
        )
    }

    /// Tells `to` that two-factor sign-in was turned on.
    #[must_use]
    pub fn two_factor_enabled(&self, to: &str) -> Mail {
        compose(
            to,
            "Two-factor sign-in is on",
            &[
                "Two-factor sign-in was just turned on for your account: signing in now also takes a code from your authenticator app, or one of your recovery codes.",
                "If it wasn't you, reset your password now.",
            ],
            None,
        )
    }

    /// Tells `to` that two-factor sign-in was turned off.
    #[must_use]
    pub fn two_factor_disabled(&self, to: &str) -> Mail {
        compose(
            to,
            "Two-factor sign-in is off",
            &[
                "Two-factor sign-in was just turned off for your account: signing in now takes only your password.",
                "If it wasn't you, reset your password now.",
            ],
            None,
        )
    }

    /// Invites `to` to set a password for the account made for them (valid
    /// 7 days).
    #[must_use]
    pub fn invitation(&self, to: &str, token: &str) -> Mail {
        compose(
            to,
            "You're invited",
            &[
                "An account was created for you. Choose your password with the link below to sign in; it is valid for 7 days.",
                "If you weren't expecting this, ignore this email.",
            ],
            Some(Action {
                label: "Accept invitation",
                url: self.link(ACCEPT_INVITATION_PATH, token),
            }),
        )
    }
}

/// A message to `to`: the paragraphs, then the action's link, in both
/// bodies. The HTML body escapes every value.
fn compose(to: &str, subject: &str, paragraphs: &[&str], action: Option<Action<'_>>) -> Mail {
    let mut text = format!("{subject}\n\n");
    let mut html = format!(
        "<!doctype html>\n<html><body>\n<h1>{}</h1>\n",
        escape(subject)
    );
    for (index, paragraph) in paragraphs.iter().enumerate() {
        text.push_str(paragraph);
        text.push_str("\n\n");
        html.push_str(&format!("<p>{}</p>\n", escape(paragraph)));
        if index == 0
            && let Some(action) = &action
        {
            text.push_str(&format!("{}: {}\n\n", action.label, action.url));
            let url = escape(&action.url);
            html.push_str(&format!(
                "<p><a href=\"{url}\">{}</a></p>\n<p>Or open this link: {url}</p>\n",
                escape(action.label)
            ));
        }
    }
    html.push_str("</body></html>\n");
    Mail {
        to: to.to_owned(),
        subject: subject.to_owned(),
        text: text.trim_end().to_owned() + "\n",
        html,
    }
}

/// `value` safe inside HTML text and a quoted attribute.
fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            c => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "abc-DEF_123";

    #[test]
    fn links_carry_the_token_under_the_app_url() {
        let templates = Templates::new("https://app.example.com/");
        let mail = templates.verification("ana@example.com", TOKEN);
        let link = "https://app.example.com/verify-email?token=abc-DEF_123";
        assert_eq!(mail.to, "ana@example.com");
        assert_eq!(mail.subject, "Verify your email address");
        assert!(mail.text.contains(link), "{}", mail.text);
        assert!(
            mail.html.contains(&format!("href=\"{link}\"")),
            "{}",
            mail.html
        );

        for (mail, path) in [
            (
                templates.password_reset("a@example.com", TOKEN),
                "/reset-password",
            ),
            (
                templates.email_change("a@example.com", TOKEN),
                "/confirm-email-change",
            ),
            (
                templates.invitation("a@example.com", TOKEN),
                "/accept-invitation",
            ),
        ] {
            let link = format!("https://app.example.com{path}?token={TOKEN}");
            assert!(mail.text.contains(&link), "{}", mail.text);
        }
    }

    #[test]
    fn notices_carry_no_link_and_escape_their_values() {
        let templates = Templates::new("http://localhost:8080");
        let mail = templates.email_change_notice("old@example.com", "<new>@example.com");
        assert!(!mail.text.contains("token="));
        assert!(mail.text.contains("<new>@example.com"));
        assert!(mail.html.contains("&lt;new&gt;@example.com"));
        assert!(!mail.html.contains("<new>"));

        let mail = templates.password_changed("ana@example.com");
        assert_eq!(mail.subject, "Your password was changed");
        assert!(!mail.text.contains("token="));

        for (mail, subject) in [
            (
                templates.two_factor_enabled("ana@example.com"),
                "Two-factor sign-in is on",
            ),
            (
                templates.two_factor_disabled("ana@example.com"),
                "Two-factor sign-in is off",
            ),
        ] {
            assert_eq!(mail.to, "ana@example.com");
            assert_eq!(mail.subject, subject);
            assert!(!mail.text.contains("token="));
        }
    }
}
