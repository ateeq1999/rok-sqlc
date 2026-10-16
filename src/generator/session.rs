//! `db::session`: helpers that set the tenant and acting-user settings
//! (ADR 0010) for the rest of the caller's transaction, for row-level
//! security policies and audit triggers to read.

use crate::Conventions;

use super::sql::str_literal;
use super::{HEADER, push_doc};

/// Renders `session.rs`, or `None` when neither `tenant_setting` nor
/// `user_setting` is configured.
pub fn render(conventions: &Conventions) -> Option<String> {
    let helpers = [
        (
            "set_tenant",
            conventions.tenant_setting.as_deref(),
            "row-level security policies see the tenant",
        ),
        (
            "set_user",
            conventions.user_setting.as_deref(),
            "audit triggers see the acting user",
        ),
    ];
    if helpers.iter().all(|(_, setting, _)| setting.is_none()) {
        return None;
    }
    let mut output = format!("{HEADER}\n");
    output.push_str(
        "//! Transaction-local settings for row-level security policies and audit\n//! triggers.\n",
    );
    for (name, setting, purpose) in helpers {
        let Some(setting) = setting else {
            continue;
        };
        output.push('\n');
        push_doc(
            &mut output,
            0,
            &[
                &format!("Sets `{setting}` to `id` until the current transaction ends"),
                &format!("(`set_config(..., true)`), so {purpose}."),
                "Outside a transaction the setting lasts for this one statement only.",
            ],
        );
        output.push_str(&format!(
            "pub async fn {name}(\n    connection: &mut sqlx::PgConnection,\n    id: impl std::fmt::Display,\n) -> Result<(), sqlx::Error> {{\n"
        ));
        let sql = format!(
            "SELECT set_config('{}', $1, true)",
            setting.replace('\'', "''")
        );
        output.push_str(&format!("    sqlx::query({})\n", str_literal(&sql)));
        output.push_str("        .bind(id.to_string())\n");
        output.push_str("        .execute(connection)\n");
        output.push_str("        .await?;\n");
        output.push_str("    Ok(())\n}\n");
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_without_settings() {
        assert_eq!(render(&Conventions::default()), None);
    }

    #[test]
    fn quotes_the_setting_as_a_sql_literal() {
        let conventions = Conventions {
            user_setting: Some("app.o'user".to_owned()),
            ..Conventions::default()
        };
        let rendered = render(&conventions).unwrap();
        assert!(rendered.contains("sqlx::query(\"SELECT set_config('app.o''user', $1, true)\")"));
        assert!(!rendered.contains("set_tenant"));
    }
}
