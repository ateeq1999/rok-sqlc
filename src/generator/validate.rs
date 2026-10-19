//! Validation rules (ADR 0015): the `#[validate(...)]` attributes
//! `derive = ["validate"]` puts on create input and update patch fields,
//! derived from each column's declared length and range checks.

use crate::schema::{BaseType, Column, ValueRange};

use super::doc_name;
use super::emit::MAX_WIDTH;
use super::sql::str_literal;

/// Appends `validator::ValidateLength` for the typed id `name` over text,
/// measuring the text inside, so a `length` rule applies through the id.
pub fn push_measured_id(output: &mut String, name: &str) {
    output.push_str(&format!(
        "impl validator::ValidateLength<u64> for {name} {{\n    fn length(&self) -> Option<u64> {{\n        validator::ValidateLength::length(&self.0)\n    }}\n}}\n\n"
    ));
}

/// Appends `validator::ValidateRange<inner>` for the typed id `name` over
/// the integer `inner`, comparing the number inside, so a `range` rule
/// applies through the id.
pub fn push_ranged_id(output: &mut String, name: &str, inner: &str) {
    output.push_str(&format!(
        "impl validator::ValidateRange<{inner}> for {name} {{\n    fn greater_than(&self, max: {inner}) -> Option<bool> {{\n        validator::ValidateRange::greater_than(&self.0, max)\n    }}\n\n    fn less_than(&self, min: {inner}) -> Option<bool> {{\n        validator::ValidateRange::less_than(&self.0, min)\n    }}\n}}\n\n"
    ));
}

/// rustfmt's default `attr_fn_like_width`: the arguments of a rule longer
/// than this go one per line.
const ATTR_FN_LIKE_WIDTH: usize = 70;

/// Appends the column's validation rule, if it has one, as a field
/// attribute. Patch fields (`Option<T>`, `Option<Option<T>>`) are checked
/// only when a value is present, which `validator` does for any `Option`.
pub fn push_field_rule(output: &mut String, column: &Column) {
    if let Some(rule) = length_rule(column).or_else(|| range_rule(column)) {
        push_rule(output, rule.kind.name(), &rule.args);
    }
}

/// A validation rule: its kind and its arguments, `message` last.
struct Rule {
    kind: RuleKind,
    args: Vec<String>,
}

/// The kinds of [`Rule`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleKind {
    /// validator's `length`.
    Length,
    /// validator's `range`.
    Range,
    /// A decimal column's range: a `custom` rule calling the function
    /// [`push_decimal_checks`] renders, since validator's `range` cannot
    /// compare a `rust_decimal::Decimal`.
    DecimalRange,
}

impl RuleKind {
    /// The rule's name inside `#[validate(...)]`.
    fn name(self) -> &'static str {
        match self {
            RuleKind::Length => "length",
            RuleKind::Range => "range",
            RuleKind::DecimalRange => "custom",
        }
    }
}

/// `rust_decimal::Decimal`, as the functions checking a decimal column's
/// range rule write it.
const DECIMAL: &str = "rust_decimal::Decimal";

/// The `range` rule of a numeric column, from its range checks. A typed id
/// over an integer implements `validator::ValidateRange` (see
/// [`push_ranged_id`]), so its field takes the rule too; a typed id over a
/// decimal gets none, since the decimal check takes a `Decimal`.
fn range_rule(column: &Column) -> Option<Rule> {
    let range = column.value_range.as_ref()?;
    let base = column.base_type();
    if column.typed_id.is_some() && base == BaseType::Decimal {
        return None;
    }
    let (float, decimal) = (base == BaseType::Float, base == BaseType::Decimal);
    if !(float || decimal || base == BaseType::Integer) {
        return None;
    }
    let mut args = Vec::new();
    if !decimal {
        // A float field compares with float literals only.
        let literal = |value: &str| {
            if float && !value.contains('.') {
                format!("{value}.0")
            } else {
                value.to_owned()
            }
        };
        for (name, bound) in [("min", &range.min), ("max", &range.max)] {
            if let Some(bound) = bound {
                let exclusive = if bound.exclusive { "exclusive_" } else { "" };
                args.push(format!("{exclusive}{name} = {}", literal(&bound.value)));
            }
        }
    } else {
        args.push(format!("function = \"{}\"", decimal_check_name(column)));
        args.push("code = \"range\"".to_owned());
    }
    let message = format!("{} must be {}", column.sql_name, range_phrase(range));
    args.push(format!("message = {}", str_literal(&message)));
    let kind = if decimal {
        RuleKind::DecimalRange
    } else {
        RuleKind::Range
    };
    Some(Rule { kind, args })
}

/// `"between 1 and 5"`, `"at least 0"`, `"greater than 0 and at most 1"`.
fn range_phrase(range: &ValueRange) -> String {
    let lower = range.min.as_ref().map(|bound| {
        let word = if bound.exclusive {
            "greater than"
        } else {
            "at least"
        };
        format!("{word} {}", bound.value)
    });
    let upper = range.max.as_ref().map(|bound| {
        let word = if bound.exclusive {
            "less than"
        } else {
            "at most"
        };
        format!("{word} {}", bound.value)
    });
    match (&range.min, &range.max) {
        (Some(min), Some(max)) if !min.exclusive && !max.exclusive => {
            format!("between {} and {}", min.value, max.value)
        }
        _ => [lower, upper]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" and "),
    }
}

/// The function checking a decimal column's range rule.
fn decimal_check_name(column: &Column) -> String {
    format!("{}_in_range", column.rust_name)
}

/// Appends the functions checking the range rules of the decimal `columns`
/// (the input and patch fields), each once.
pub fn push_decimal_checks<'a>(output: &mut String, columns: impl IntoIterator<Item = &'a Column>) {
    let mut done: Vec<&str> = Vec::new();
    for column in columns {
        if done.contains(&column.sql_name.as_str())
            || range_rule(column).is_none_or(|rule| rule.kind != RuleKind::DecimalRange)
        {
            continue;
        }
        done.push(&column.sql_name);
        let Some(range) = &column.value_range else {
            continue;
        };
        let name = decimal_check_name(column);
        output.push_str(&format!(
            "\n/// The range rule of the {} column.\n",
            doc_name(&column.sql_name)
        ));
        let one_line =
            format!("fn {name}(value: &{DECIMAL}) -> Result<(), validator::ValidationError> {{");
        if one_line.len() <= MAX_WIDTH {
            output.push_str(&one_line);
            output.push('\n');
        } else {
            output.push_str(&format!(
                "fn {name}(\n    value: &{DECIMAL},\n) -> Result<(), validator::ValidationError> {{\n"
            ));
        }
        // Each bound is a local named as validator's `range` names its
        // param, so the error carries the same `params`.
        let mut bounds = Vec::new();
        let mut terms = Vec::new();
        for (name, bound, outside, outside_exclusive) in [
            ("min", &range.min, "<", "<="),
            ("max", &range.max, ">", ">="),
        ] {
            let Some(bound) = bound else { continue };
            let (param, op) = if bound.exclusive {
                (format!("exclusive_{name}"), outside_exclusive)
            } else {
                (name.to_owned(), outside)
            };
            output.push_str(&format!(
                "    let {param} = {};\n",
                decimal_expr(&bound.value)
            ));
            terms.push(format!("*value {op} {param}"));
            bounds.push(param);
        }
        push_or_condition(output, &terms);
        output.push_str("        let mut error = validator::ValidationError::new(\"range\");\n");
        for param in &bounds {
            output.push_str(&format!(
                "        error.add_param(\"{param}\".into(), &{param});\n"
            ));
        }
        output.push_str("        return Err(error);\n    }\n    Ok(())\n}\n");
    }
}

/// `    if a || b {`, wrapped as rustfmt wraps a condition too long for one
/// line.
fn push_or_condition(output: &mut String, terms: &[String]) {
    let one_line = format!("    if {} {{", terms.join(" || "));
    if one_line.len() <= MAX_WIDTH {
        output.push_str(&one_line);
        output.push('\n');
        return;
    }
    output.push_str(&format!("    if {}\n", terms[0]));
    for term in &terms[1..] {
        output.push_str(&format!("        || {term}\n"));
    }
    output.push_str("    {\n");
}

/// A decimal literal (`-99.95`) as the exact `Decimal` it denotes.
fn decimal_expr(value: &str) -> String {
    let scale = value
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    let digits = value.replace('.', "");
    let mantissa: i128 = digits.parse().expect("a range bound is a number literal");
    format!("{DECIMAL}::from_i128_with_scale({mantissa}, {scale})")
}

/// `"{name}(args)"` inside `#[validate(...)]`, laid out as rustfmt lays it
/// out: on one line when the arguments fit, otherwise one per line.
pub fn push_rule(output: &mut String, kind: &str, args: &[String]) {
    let joined = args.join(", ");
    if joined.len() <= ATTR_FN_LIKE_WIDTH {
        output.push_str(&format!("    #[validate({kind}({joined}))]\n"));
        return;
    }
    output.push_str(&format!("    #[validate({kind}(\n"));
    for arg in args {
        output.push_str(&format!("        {arg},\n"));
    }
    // rustfmt leaves no comma after the last argument of an attribute.
    output.truncate(output.len() - 2);
    output.push_str("\n    ))]\n");
}

/// The `length` rule of a text column: its declared length and length
/// checks, the tightest bounds when both apply. Lengths count characters,
/// as both PostgreSQL and `validator` count them.
fn length_rule(column: &Column) -> Option<Rule> {
    if !holds_text(column) {
        return None;
    }
    let checked = column.length_range;
    let min = checked.and_then(|range| range.min);
    let max = [column.declared_length, checked.and_then(|range| range.max)]
        .into_iter()
        .flatten()
        .min();
    let name = &column.sql_name;
    let (mut args, message) = match (min, max) {
        (None, None) => return None,
        (Some(min), Some(max)) if min == max => (
            vec![format!("equal = {min}")],
            format!("{name} must be exactly {}", characters(min)),
        ),
        (Some(min), Some(max)) => (
            vec![format!("min = {min}"), format!("max = {max}")],
            format!("{name} must be between {min} and {}", characters(max)),
        ),
        (Some(min), None) => (
            vec![format!("min = {min}")],
            format!("{name} must be at least {}", characters(min)),
        ),
        (None, Some(max)) => (
            vec![format!("max = {max}")],
            format!("{name} must be at most {}", characters(max)),
        ),
    };
    args.push(format!("message = {}", str_literal(&message)));
    Some(Rule {
        kind: RuleKind::Length,
        args,
    })
}

/// Whether the field holds the column's text: a `String`, or a typed id
/// over one, which implements `validator::ValidateLength`.
fn holds_text(column: &Column) -> bool {
    column.base_type() == BaseType::Text
}

/// `"1 character"`, `"80 characters"`.
fn characters(count: u32) -> String {
    if count == 1 {
        "1 character".to_owned()
    } else {
        format!("{count} characters")
    }
}
