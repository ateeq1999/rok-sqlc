//! String rules (`[api.rules]`) on the create and update bodies: a field's
//! doc lines, its transforms (applied while the body is read, through
//! `deserialize_with`), the OpenAPI keywords utoipa can carry, and its
//! checks, as `validator`'s own rules where they mean what VineJS's do
//! (`email`, `ip`), otherwise as `custom` rules calling a generated
//! function. The error code of every check is its VineJS name.

use heck::ToSnakeCase;

use super::layout::push_field_attr;
use crate::api_rules::{Extras, Rule};
use crate::generator::sql::str_literal;
use crate::generator::validate::push_rule;
use crate::generator::{MAX_WIDTH, push_doc};
use crate::schema::Column;

/// The doc lines a field's rules add: its transforms and its rules, after a
/// blank line. The OpenAPI document shows them as the field's description.
pub fn push_doc_lines(output: &mut String, rules: &[Rule]) {
    if rules.is_empty() {
        return;
    }
    push_doc(output, 4, &[""]);
    let names = |transforms: bool| -> Vec<String> {
        rules
            .iter()
            .filter(|rule| rule.is_transform() == transforms)
            .map(|rule| format!("`{}`", rule.display()))
            .collect()
    };
    for (label, names) in [("Transforms:", names(true)), ("Rules:", names(false))] {
        if names.is_empty() {
            continue;
        }
        // One line when it fits, otherwise wrapped at the maximum width.
        let mut line = label.to_owned();
        for (index, name) in names.iter().enumerate() {
            let last = index + 1 == names.len();
            let word = if last {
                format!("{name}.")
            } else {
                format!("{name},")
            };
            if 8 + line.len() + 1 + word.len() > MAX_WIDTH {
                push_doc(output, 4, &[&line]);
                line = word;
            } else {
                line.push(' ');
                line.push_str(&word);
            }
        }
        push_doc(output, 4, &[&line]);
    }
}

/// The `#[serde(...)]` of a field whose column has transforms: deserialized
/// through `<column>_transformed`, defaulted when optional on create (the
/// update body is defaulted as a whole). Returns whether it was pushed.
pub fn push_transformed(
    output: &mut String,
    column: &Column,
    rules: &[Rule],
    create: bool,
    field_type: &str,
) -> bool {
    if !rules.iter().any(Rule::is_transform) {
        return false;
    }
    let mut args = Vec::new();
    if create && field_type.starts_with("Option<") {
        args.push("default".to_owned());
    }
    args.push(format!(
        "deserialize_with = \"{}_transformed\"",
        column.rust_name
    ));
    // rustfmt keeps a lone argument on the attribute's line while it fits.
    let one_line = format!("    #[serde({})]", args.join(", "));
    if args.len() == 1 && one_line.len() <= MAX_WIDTH {
        output.push_str(&one_line);
        output.push('\n');
    } else {
        push_field_attr(output, "serde", &args);
    }
    true
}

/// `#[schema(...)]` with the OpenAPI keywords utoipa can carry: a format
/// (`email`, `ipv4`, `ipv6`, `uuid`; not `uri` for `url`, which
/// takes a value without a scheme), a `pattern`, and lengths.
/// The first of each keyword wins.
pub fn push_schema(output: &mut String, openapi: bool, rules: &[Rule]) {
    if !openapi {
        return;
    }
    let mut format: Option<&str> = None;
    let mut pattern: Option<String> = None;
    let (mut min, mut max): (Option<u32>, Option<u32>) = (None, None);
    for rule in rules {
        match rule {
            Rule::Email => {
                format.get_or_insert("email");
            }
            Rule::IpAddress(Some(4)) => {
                format.get_or_insert("ipv4");
            }
            Rule::IpAddress(Some(_)) => {
                format.get_or_insert("ipv6");
            }
            Rule::Uuid(_) => {
                format.get_or_insert("uuid");
            }
            Rule::Regex(regex) => {
                pattern.get_or_insert_with(|| regex.clone());
            }
            Rule::MinLength(count) => {
                min.get_or_insert(*count);
            }
            Rule::MaxLength(count) => {
                max.get_or_insert(*count);
            }
            Rule::FixedLength(count) => {
                min.get_or_insert(*count);
                max.get_or_insert(*count);
            }
            _ => {}
        }
    }
    let mut args = Vec::new();
    if let Some(format) = format {
        args.push(format!("format = \"{format}\""));
    }
    if let Some(pattern) = pattern {
        args.push(format!("pattern = {}", str_literal(&pattern)));
    }
    if let Some(min) = min {
        args.push(format!("min_length = {min}"));
    }
    if let Some(max) = max {
        args.push(format!("max_length = {max}"));
    }
    if !args.is_empty() {
        push_field_attr(output, "schema", &args);
    }
}

/// The `#[validate(...)]` of each check of `column`.
pub fn push_checks(output: &mut String, column: &Column, rules: &[Rule]) {
    let name = &column.sql_name;
    for rule in rules.iter().filter(|rule| !rule.is_transform()) {
        let mut args = Vec::new();
        let kind = match rule {
            Rule::Email => "email",
            Rule::IpAddress(version) => {
                if let Some(version) = version {
                    args.push(format!("v{version}"));
                }
                "ip"
            }
            _ => {
                args.push(format!("function = \"{}\"", function_name(column, rule)));
                "custom"
            }
        };
        args.push(format!("code = \"{}\"", rule.name()));
        args.push(format!("message = {}", str_literal(&message(name, rule))));
        push_rule(output, kind, &args);
    }
}

/// The function checking `rule` on `column`: `handle_min_length`.
pub(super) fn function_name(column: &Column, rule: &Rule) -> String {
    format!("{}_{}", column.rust_name, rule.name().to_snake_case())
}

/// The English message of a failed check, naming the column.
fn message(name: &str, rule: &Rule) -> String {
    let characters = |count: u32| {
        if count == 1 {
            "1 character".to_owned()
        } else {
            format!("{count} characters")
        }
    };
    let what = match rule {
        Rule::Email => "be an email address".to_owned(),
        Rule::Url => "be a URL".to_owned(),
        Rule::Uuid(_) => "be a UUID".to_owned(),
        Rule::Ulid => "be a ULID".to_owned(),
        Rule::IpAddress(None) => "be an IP address".to_owned(),
        Rule::IpAddress(Some(version)) => format!("be an IPv{version} address"),
        Rule::Mobile { .. } => "be a mobile phone number".to_owned(),
        Rule::Alpha(extras) => format!("contain only {}", allowed("letters", extras)),
        Rule::AlphaNumeric(extras) => {
            format!("contain only {}", allowed("letters, digits", extras))
        }
        Rule::Ascii => "contain only ASCII characters".to_owned(),
        Rule::Regex(_) => "match the expected format".to_owned(),
        Rule::In(values) => format!("be one of {}", values.join(", ")),
        Rule::NotIn(values) => format!("not be one of {}", values.join(", ")),
        Rule::StartsWith(text) => format!("start with {text}"),
        Rule::EndsWith(text) => format!("end with {text}"),
        Rule::MinLength(count) => format!("be at least {}", characters(*count)),
        Rule::MaxLength(count) => format!("be at most {}", characters(*count)),
        Rule::FixedLength(count) => format!("be exactly {}", characters(*count)),
        Rule::HexCode => "be a hex color code".to_owned(),
        Rule::Iban => "be an IBAN".to_owned(),
        Rule::Jwt => "be a JWT".to_owned(),
        Rule::PostalCode(_) => "be a postal code".to_owned(),
        Rule::CreditCard(_) => "be a credit card number".to_owned(),
        Rule::Coordinates => "be latitude and longitude coordinates".to_owned(),
        Rule::Trim | Rule::ToLowerCase | Rule::ToUpperCase | Rule::NormalizeEmail => String::new(),
    };
    format!("{name} must {what}")
}

/// `"letters, digits, spaces and dashes"`.
fn allowed(base: &str, extras: &Extras) -> String {
    let mut kinds: Vec<&str> = base.split(", ").collect();
    for (on, kind) in [
        (extras.spaces, "spaces"),
        (extras.dashes, "dashes"),
        (extras.underscores, "underscores"),
    ] {
        if on {
            kinds.push(kind);
        }
    }
    match kinds.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => kinds.concat(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_name_the_column() {
        assert_eq!(
            message(
                "handle",
                &Rule::AlphaNumeric(Extras {
                    spaces: false,
                    dashes: true,
                    underscores: true
                })
            ),
            "handle must contain only letters, digits, dashes and underscores"
        );
        assert_eq!(
            message("code", &Rule::FixedLength(1)),
            "code must be exactly 1 character"
        );
    }
}
