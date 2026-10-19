//! String rules (`[api.rules]`): VineJS's string rules and transforms by
//! name, set on text columns, and checked against the schema by [`check`].
//! See <https://vinejs.dev/docs/types/string>.

use std::collections::BTreeMap;

use crate::schema::{BaseType, Column, Table};

/// One string rule or transform of [`ApiConfig::rules`](crate::ApiConfig::rules),
/// as the config file writes it: a string, or an inline table where a
/// string can't carry the argument.
///
/// ```
/// use rok_sqlc::ApiRule;
/// let rules = vec![
///     ApiRule::from("trim"),
///     ApiRule::from("alphaNumeric(allowDashes)"),
///     ApiRule::Regex("^[a-z0-9-]+$".into()),
/// ];
/// # let _ = rules;
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ApiRule {
    /// A rule or transform written as text: `"email"`, `"minLength(3)"`,
    /// `"in(SA, AE, EG)"`.
    Text(String),
    /// `{ regex = "^[a-z0-9-]+$" }`: the value matches the pattern, in the
    /// `regex` crate's syntax.
    Regex(String),
    /// `{ in = ["a,b", "c"] }`: the value is one of these.
    In(Vec<String>),
    /// `{ notIn = ["a,b", "c"] }`: the value is none of these.
    NotIn(Vec<String>),
    /// `{ startsWith = "a,b" }`: the value starts with this.
    StartsWith(String),
    /// `{ endsWith = "a,b" }`: the value ends with this.
    EndsWith(String),
}

impl From<&str> for ApiRule {
    fn from(text: &str) -> Self {
        ApiRule::Text(text.to_owned())
    }
}

/// The characters `alpha` and `alphaNumeric` take besides letters (and
/// digits).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Extras {
    /// `allowSpaces`: whitespace.
    pub spaces: bool,
    /// `allowDashes`: `-`.
    pub dashes: bool,
    /// `allowUnderscores`: `_`.
    pub underscores: bool,
}

/// A string rule or transform, its arguments checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Rule {
    Trim,
    ToLowerCase,
    ToUpperCase,
    NormalizeEmail,
    Email,
    Url,
    /// The versions allowed; none allows any.
    Uuid(Vec<u8>),
    Ulid,
    /// `4`, `6`, or either.
    IpAddress(Option<u8>),
    /// The locales; none for any locale rok-sqlc carries ([`MOBILE_LOCALES`]).
    Mobile {
        locales: Vec<String>,
        strict: bool,
    },
    Alpha(Extras),
    AlphaNumeric(Extras),
    Ascii,
    Regex(String),
    In(Vec<String>),
    NotIn(Vec<String>),
    StartsWith(String),
    EndsWith(String),
    MinLength(u32),
    MaxLength(u32),
    FixedLength(u32),
    HexCode,
    Iban,
    Jwt,
    /// The countries allowed.
    PostalCode(Vec<String>),
    /// The providers allowed; none allows any.
    CreditCard(Vec<String>),
    Coordinates,
}

/// The validation rules, by VineJS name.
pub(crate) const RULES: [&str; 23] = [
    "email",
    "url",
    "uuid",
    "ulid",
    "ipAddress",
    "mobile",
    "alpha",
    "alphaNumeric",
    "ascii",
    "regex",
    "in",
    "notIn",
    "startsWith",
    "endsWith",
    "minLength",
    "maxLength",
    "fixedLength",
    "hexCode",
    "iban",
    "jwt",
    "postalCode",
    "creditCard",
    "coordinates",
];

/// The transforms, by VineJS name.
pub(crate) const TRANSFORMS: [&str; 4] = ["trim", "toLowerCase", "toUpperCase", "normalizeEmail"];

/// The locales `mobile` takes: validator.js's `isMobilePhone` patterns
/// rok-sqlc carries.
pub(crate) const MOBILE_LOCALES: [&str; 16] = [
    "ar-AE", "ar-BH", "ar-EG", "ar-JO", "ar-KW", "ar-OM", "ar-QA", "ar-SA", "de-DE", "en-CA",
    "en-GB", "en-IN", "en-US", "es-ES", "fr-FR", "tr-TR",
];

/// The countries `postalCode` takes: validator.js's `isPostalCode` patterns
/// rok-sqlc carries.
pub(crate) const POSTAL_COUNTRIES: [&str; 7] = ["CA", "DE", "ES", "FR", "GB", "SA", "US"];

/// The providers `creditCard` takes, as validator.js's `isCreditCard` names
/// them.
pub(crate) const CARD_PROVIDERS: [&str; 7] = [
    "amex",
    "dinersclub",
    "discover",
    "jcb",
    "mastercard",
    "unionpay",
    "visa",
];

impl Rule {
    /// The VineJS name: the error code of a validation rule.
    pub fn name(&self) -> &'static str {
        match self {
            Rule::Trim => "trim",
            Rule::ToLowerCase => "toLowerCase",
            Rule::ToUpperCase => "toUpperCase",
            Rule::NormalizeEmail => "normalizeEmail",
            Rule::Email => "email",
            Rule::Url => "url",
            Rule::Uuid(_) => "uuid",
            Rule::Ulid => "ulid",
            Rule::IpAddress(_) => "ipAddress",
            Rule::Mobile { .. } => "mobile",
            Rule::Alpha(_) => "alpha",
            Rule::AlphaNumeric(_) => "alphaNumeric",
            Rule::Ascii => "ascii",
            Rule::Regex(_) => "regex",
            Rule::In(_) => "in",
            Rule::NotIn(_) => "notIn",
            Rule::StartsWith(_) => "startsWith",
            Rule::EndsWith(_) => "endsWith",
            Rule::MinLength(_) => "minLength",
            Rule::MaxLength(_) => "maxLength",
            Rule::FixedLength(_) => "fixedLength",
            Rule::HexCode => "hexCode",
            Rule::Iban => "iban",
            Rule::Jwt => "jwt",
            Rule::PostalCode(_) => "postalCode",
            Rule::CreditCard(_) => "creditCard",
            Rule::Coordinates => "coordinates",
        }
    }

    /// Whether the rule changes the value rather than checks it.
    pub fn is_transform(&self) -> bool {
        matches!(
            self,
            Rule::Trim | Rule::ToLowerCase | Rule::ToUpperCase | Rule::NormalizeEmail
        )
    }

    /// Whether the generated check needs the `regex` crate.
    pub fn needs_regex(&self) -> bool {
        matches!(
            self,
            Rule::Regex(_) | Rule::Mobile { .. } | Rule::PostalCode(_)
        )
    }

    /// The rule as the config file writes it, for docs: `minLength(3)`.
    pub fn display(&self) -> String {
        let extras = |extras: &Extras| {
            [
                (extras.spaces, "allowSpaces"),
                (extras.dashes, "allowDashes"),
                (extras.underscores, "allowUnderscores"),
            ]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, name)| name.to_owned())
            .collect::<Vec<_>>()
        };
        let args: Vec<String> = match self {
            Rule::Uuid(versions) => versions.iter().map(u8::to_string).collect(),
            Rule::IpAddress(version) => version.iter().map(u8::to_string).collect(),
            Rule::Mobile { locales, strict } => {
                let mut args = locales.clone();
                if *strict {
                    args.push("strictMode".to_owned());
                }
                args
            }
            Rule::Alpha(e) | Rule::AlphaNumeric(e) => extras(e),
            Rule::Regex(pattern) => vec![pattern.clone()],
            Rule::In(values) | Rule::NotIn(values) => values.clone(),
            Rule::PostalCode(values) | Rule::CreditCard(values) => values.clone(),
            Rule::StartsWith(text) | Rule::EndsWith(text) => vec![text.clone()],
            Rule::MinLength(n) | Rule::MaxLength(n) | Rule::FixedLength(n) => vec![n.to_string()],
            _ => Vec::new(),
        };
        if args.is_empty() {
            self.name().to_owned()
        } else {
            format!("{}({})", self.name(), args.join(", "))
        }
    }
}

/// Parses `rule`. The error completes "`api.rules.<table>.<column>` in the
/// configuration has ".
pub(crate) fn parse(rule: &ApiRule) -> Result<Rule, String> {
    match rule {
        ApiRule::Regex(pattern) => {
            regex_syntax::Parser::new()
                .parse(pattern)
                .map_err(|error| {
                    format!("the pattern `{pattern}`, which is no regular expression: {error}")
                })?;
            Ok(Rule::Regex(pattern.clone()))
        }
        ApiRule::In(values) | ApiRule::NotIn(values) if values.is_empty() => {
            Err("an inline `in` or `notIn` with no values".to_owned())
        }
        ApiRule::In(values) => Ok(Rule::In(values.clone())),
        ApiRule::NotIn(values) => Ok(Rule::NotIn(values.clone())),
        ApiRule::StartsWith(text) => Ok(Rule::StartsWith(text.clone())),
        ApiRule::EndsWith(text) => Ok(Rule::EndsWith(text.clone())),
        ApiRule::Text(text) => parse_text(text),
    }
}

/// Parses a rule written as text: `name` or `name(arg, ...)`.
fn parse_text(text: &str) -> Result<Rule, String> {
    let trimmed = text.trim();
    let (name, args) = match trimmed.split_once('(') {
        Some((name, rest)) => {
            let Some(inner) = rest.strip_suffix(')') else {
                return Err(format!("`{text}`, which has no closing `)`"));
            };
            let args: Vec<String> = inner
                .split(',')
                .map(|arg| arg.trim().to_owned())
                .filter(|arg| !arg.is_empty())
                .collect();
            (name.trim(), args)
        }
        None => (trimmed, Vec::new()),
    };
    let bad = |takes: &str| format!("`{text}`, but `{name}` takes {takes}");
    let none = |rule: Rule| {
        if args.is_empty() {
            Ok(rule)
        } else {
            Err(bad("no arguments"))
        }
    };
    match name {
        "trim" => none(Rule::Trim),
        "toLowerCase" => none(Rule::ToLowerCase),
        "toUpperCase" => none(Rule::ToUpperCase),
        "normalizeEmail" => none(Rule::NormalizeEmail),
        "email" => none(Rule::Email),
        "url" => none(Rule::Url),
        "ulid" => none(Rule::Ulid),
        "ascii" => none(Rule::Ascii),
        "hexCode" => none(Rule::HexCode),
        "iban" => none(Rule::Iban),
        "jwt" => none(Rule::Jwt),
        "coordinates" => none(Rule::Coordinates),
        "uuid" => {
            let versions: Option<Vec<u8>> = args
                .iter()
                .map(|arg| arg.parse().ok().filter(|v| (1..=8).contains(v)))
                .collect();
            versions
                .map(Rule::Uuid)
                .ok_or_else(|| bad("UUID versions from 1 to 8, such as `uuid(4, 7)`"))
        }
        "ipAddress" => match args.as_slice() {
            [] => Ok(Rule::IpAddress(None)),
            [version] if version == "4" || version == "6" => {
                Ok(Rule::IpAddress(version.parse().ok()))
            }
            _ => Err(bad("one version, `4` or `6`, or none")),
        },
        "mobile" => {
            let strict = args.iter().any(|arg| arg == "strictMode");
            let locales: Vec<String> = args
                .iter()
                .filter(|arg| *arg != "strictMode")
                .cloned()
                .collect();
            if !locales.iter().all(|l| MOBILE_LOCALES.contains(&l.as_str())) {
                return Err(bad(&format!(
                    "locales, such as `mobile(ar-SA)`, or none for any, and `strictMode`; the locales are {}",
                    quoted(&MOBILE_LOCALES)
                )));
            }
            Ok(Rule::Mobile { locales, strict })
        }
        "alpha" | "alphaNumeric" => {
            let mut extras = Extras::default();
            for arg in &args {
                match arg.as_str() {
                    "allowSpaces" => extras.spaces = true,
                    "allowDashes" => extras.dashes = true,
                    "allowUnderscores" => extras.underscores = true,
                    _ => return Err(bad("`allowSpaces`, `allowDashes` and `allowUnderscores`")),
                }
            }
            Ok(if name == "alpha" {
                Rule::Alpha(extras)
            } else {
                Rule::AlphaNumeric(extras)
            })
        }
        "regex" => Err(format!(
            "`{text}`, but `regex` takes its pattern in an inline table: `{{ regex = \"^[a-z0-9-]+$\" }}`"
        )),
        "in" | "notIn" if args.is_empty() => Err(bad(&format!(
            "one or more values, such as `{name}(SA, AE)`, or an inline table for values with commas: `{{ {name} = [\"a,b\", \"c\"] }}`"
        ))),
        "in" => Ok(Rule::In(args)),
        "notIn" => Ok(Rule::NotIn(args)),
        "startsWith" | "endsWith" => match args.as_slice() {
            [arg] if name == "startsWith" => Ok(Rule::StartsWith(arg.clone())),
            [arg] => Ok(Rule::EndsWith(arg.clone())),
            _ => Err(bad(&format!(
                "one text, such as `{name}(ab)`, or an inline table for text with commas or spaces: `{{ {name} = \"a, b\" }}`"
            ))),
        },
        "minLength" | "maxLength" | "fixedLength" => {
            let count = match args.as_slice() {
                [arg] => arg.parse::<u32>().ok(),
                _ => None,
            };
            let Some(count) = count else {
                return Err(bad(&format!("a number of characters, such as `{name}(3)`")));
            };
            Ok(match name {
                "minLength" => Rule::MinLength(count),
                "maxLength" => Rule::MaxLength(count),
                _ => Rule::FixedLength(count),
            })
        }
        "postalCode" => {
            if args.is_empty() || !args.iter().all(|c| POSTAL_COUNTRIES.contains(&c.as_str())) {
                return Err(bad(&format!(
                    "one or more countries, such as `postalCode(SA)`; the countries are {}",
                    quoted(&POSTAL_COUNTRIES)
                )));
            }
            Ok(Rule::PostalCode(args))
        }
        "creditCard" => {
            if !args.iter().all(|p| CARD_PROVIDERS.contains(&p.as_str())) {
                return Err(bad(&format!(
                    "providers, or none for any; the providers are {}",
                    quoted(&CARD_PROVIDERS)
                )));
            }
            Ok(Rule::CreditCard(args))
        }
        "confirmed" | "sameAs" | "notSameAs" => Err(format!(
            "`{name}`, which rok-sqlc leaves out: cross-field rules belong in hand-written request types"
        )),
        "activeUrl" => Err(
            "`activeUrl`, which rok-sqlc leaves out: it looks up DNS records while validating"
                .to_owned(),
        ),
        _ => Err(format!(
            "`{name}`, which is no string rule; the rules are {}, and the transforms {}",
            quoted(&RULES),
            quoted(&TRANSFORMS)
        )),
    }
}

/// `` `a`, `b` and `c` ``.
fn quoted(names: &[&str]) -> String {
    let quoted: Vec<String> = names.iter().map(|name| format!("`{name}`")).collect();
    match quoted.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => quoted.concat(),
    }
}

/// `` `api.rules.<table>.<column>` ``, quoting a part that needs it.
fn location(table: &str, column: &str) -> String {
    let part = |name: &str| {
        if name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            name.to_owned()
        } else {
            format!("\"{name}\"")
        }
    };
    format!("`api.rules.{}.{}`", part(table), part(column))
}

/// Parses the rules of one column, `key` naming its table: each rule well
/// formed, none twice.
pub(crate) fn parse_column(
    key: &str,
    column: &str,
    rules: &[ApiRule],
) -> Result<Vec<Rule>, String> {
    let mut parsed: Vec<Rule> = Vec::new();
    for rule in rules {
        let rule = parse(rule).map_err(|error| {
            format!("{} in the configuration has {error}", location(key, column))
        })?;
        if parsed.iter().any(|other| other.name() == rule.name()) {
            return Err(format!(
                "{} in the configuration has `{}` twice",
                location(key, column),
                rule.name()
            ));
        }
        parsed.push(rule);
    }
    Ok(parsed)
}

/// Why `column` can't take string rules, if it can't: it isn't a plain
/// text column (`String`), or generated code never writes it.
pub(crate) fn unfit(column: &Column, managed: bool) -> Option<&'static str> {
    if column.base_type() != BaseType::Text || column.typed_id.is_some() {
        Some("which is not a text column, so string rules can't check it")
    } else if managed {
        Some("a managed column, which generated code never writes")
    } else if column.is_generated {
        Some("a generated column, which generated code never writes")
    } else {
        None
    }
}

/// The rules of each column of `table` in `rules`, the `[api.rules]` entry
/// naming it (after [`check`](crate::api_config) passed).
pub(crate) fn of_table<'a>(
    table: &'a Table,
    rules: Option<&BTreeMap<String, Vec<ApiRule>>>,
) -> Vec<(&'a Column, Vec<Rule>)> {
    let Some(rules) = rules else {
        return Vec::new();
    };
    table
        .columns
        .iter()
        .filter_map(|column| {
            let rules = rules.get(&column.sql_name)?;
            let parsed = parse_column("", &column.sql_name, rules).ok()?;
            Some((column, parsed))
        })
        .collect()
}

/// Whether any string rule of `api` matches a regular expression, which
/// the generated check does with the `regex` crate.
pub(crate) fn uses_regex(api: &crate::ApiConfig) -> bool {
    api.rules
        .values()
        .flat_map(BTreeMap::values)
        .flatten()
        .any(|rule| parse(rule).is_ok_and(|rule| rule.needs_regex()))
}

#[cfg(test)]
mod tests;
