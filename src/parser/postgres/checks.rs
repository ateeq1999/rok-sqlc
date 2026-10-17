//! Range checks: single-column `CHECK`s against number literals, kept as
//! the value and length ranges validation rules come from (ADR 0015).

use std::cmp::Ordering;

use sqlparser::ast::{
    BinaryOperator, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, UnaryOperator, Value,
};

use super::names::ident_name;
use crate::schema::{BaseType, Column, LengthRange, NumberBound, ValueRange};

/// A range check: the column it names and the bounds it sets.
pub(super) struct RangeCheck {
    pub(super) column: String,
    bounds: Vec<Bound>,
}

/// One comparison of a range check, read as a bound on the column.
struct Bound {
    /// The bound limits the column's length in characters, not its value.
    length: bool,
    /// A lower bound (`col > n`), or else an upper one.
    lower: bool,
    /// The bound itself is not allowed (`>` or `<`).
    strict: bool,
    literal: Decimal,
}

/// The range check `expr` is, or `None` for any other shape. A range check
/// compares one column, or its `char_length`, `character_length`, or
/// `length`, with number literals: `col >= n`, `col > n`, `col <= n`,
/// `col < n`, the same with the literal on the left, `col BETWEEN a AND b`,
/// or an `AND` of these on the same column. Anything else in the
/// expression, such as `OR`, a second column, or another function, makes it
/// no range check.
pub(super) fn range_check(expr: &Expr) -> Option<RangeCheck> {
    let mut bounds = Vec::new();
    let column = collect_bounds(expr, &mut bounds)?;
    Some(RangeCheck { column, bounds })
}

/// Adds the bounds of `expr` to `bounds`, returning the column they are on.
fn collect_bounds(expr: &Expr, bounds: &mut Vec<Bound>) -> Option<String> {
    match expr {
        Expr::Nested(inner) => collect_bounds(inner, bounds),
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            let column = collect_bounds(left, bounds)?;
            (collect_bounds(right, bounds)? == column).then_some(column)
        }
        Expr::BinaryOp { left, op, right } => {
            // `n < col` bounds `col` as `col > n` does.
            let (lower, strict) = match op {
                BinaryOperator::Gt => (true, true),
                BinaryOperator::GtEq => (true, false),
                BinaryOperator::Lt => (false, true),
                BinaryOperator::LtEq => (false, false),
                _ => return None,
            };
            let ((column, length), literal, lower) = match (subject(left), literal(right)) {
                (Some(subject), Some(literal)) => (subject, literal, lower),
                _ => (subject(right)?, literal(left)?, !lower),
            };
            bounds.push(Bound {
                length,
                lower,
                strict,
                literal,
            });
            Some(column)
        }
        Expr::Between {
            expr,
            negated: false,
            low,
            high,
        } => {
            let (column, length) = subject(expr)?;
            for (lower, literal) in [(true, literal(low)?), (false, literal(high)?)] {
                bounds.push(Bound {
                    length,
                    lower,
                    strict: false,
                    literal,
                });
            }
            Some(column)
        }
        _ => None,
    }
}

/// The column a comparison side names, and whether it is the column's
/// length (`char_length(col)`, `character_length(col)`, or `length(col)`).
fn subject(expr: &Expr) -> Option<(String, bool)> {
    match expr {
        Expr::Nested(inner) => subject(inner),
        Expr::Identifier(column) => Some((ident_name(column), false)),
        Expr::Function(function)
            if function.filter.is_none()
                && function.over.is_none()
                && function.within_group.is_empty()
                && matches!(function.parameters, FunctionArguments::None) =>
        {
            let [name] = function.name.0.as_slice() else {
                return None;
            };
            let name = name.as_ident()?.value.to_ascii_lowercase();
            if !matches!(name.as_str(), "char_length" | "character_length" | "length") {
                return None;
            }
            let FunctionArguments::List(list) = &function.args else {
                return None;
            };
            let [FunctionArg::Unnamed(FunctionArgExpr::Expr(Expr::Identifier(column)))] =
                list.args.as_slice()
            else {
                return None;
            };
            (list.duplicate_treatment.is_none() && list.clauses.is_empty())
                .then(|| (ident_name(column), true))
        }
        _ => None,
    }
}

/// A number literal, with any sign in front of it.
fn literal(expr: &Expr) -> Option<Decimal> {
    match expr {
        Expr::Nested(inner) => literal(inner),
        Expr::UnaryOp {
            op: UnaryOperator::Minus,
            expr,
        } => literal(expr)?.negated(),
        Expr::UnaryOp {
            op: UnaryOperator::Plus,
            expr,
        } => literal(expr),
        Expr::Value(value) => match &value.value {
            Value::Number(text, _) => Decimal::parse(text),
            _ => None,
        },
        _ => None,
    }
}

/// Narrows `column`'s ranges to the bounds of `check`. A bound the column's
/// type cannot take (a value bound on a non-number column, a length bound on
/// a non-text one, a literal out of the type's range) is dropped: the
/// database still enforces the check, and the generated rule is only looser.
///
/// Strict bounds are made inclusive where they can be. On an integer column
/// `> n` is `>= n + 1`, and a literal with a fraction is dropped; length
/// bounds are integers too. On a float or `NUMERIC` column, a strict bound
/// stays strict, and is kept only if its literal is exactly a value of the
/// type: the nearest `f64` to `0.1` is not `0.1`, so `> 0.1` checked in
/// Rust would accept or reject a value the database does not.
pub(super) fn apply_range_check(column: &mut Column, check: RangeCheck) {
    for bound in check.bounds {
        if bound.length {
            narrow_length(column, &bound);
        } else {
            narrow_value(column, bound);
        }
    }
}

fn narrow_length(column: &mut Column, bound: &Bound) {
    if column.base_type() != BaseType::Text {
        return;
    }
    let Some(limit) = bound
        .literal
        .integer()
        .and_then(|limit| inclusive(limit, bound))
    else {
        return;
    };
    let range = column.length_range.get_or_insert(LengthRange {
        min: None,
        max: None,
    });
    if bound.lower {
        // A length is never below zero, so `>= 0` (or less) bounds nothing.
        if let Ok(min) = u32::try_from(limit)
            && min > 0
        {
            range.min = Some(range.min.map_or(min, |earlier| earlier.max(min)));
        }
    } else if let Ok(max) = u32::try_from(limit) {
        range.max = Some(range.max.map_or(max, |earlier| earlier.min(max)));
    }
    if range.min.is_none() && range.max.is_none() {
        column.length_range = None;
    }
}

fn narrow_value(column: &mut Column, bound: Bound) {
    let Some(number) = number_bound(column, &bound) else {
        return;
    };
    let range = column.value_range.get_or_insert(ValueRange {
        min: None,
        max: None,
    });
    let (slot, tighter) = if bound.lower {
        (&mut range.min, Ordering::Greater)
    } else {
        (&mut range.max, Ordering::Less)
    };
    let replace = match slot {
        None => true,
        Some(earlier) => {
            let earlier_value = Decimal::parse(&earlier.value);
            let new_value = Decimal::parse(&number.value);
            match earlier_value
                .zip(new_value)
                .and_then(|(a, b)| b.compare(&a))
            {
                Some(Ordering::Equal) => number.exclusive && !earlier.exclusive,
                Some(order) => order == tighter,
                // Literals too long to compare: keep the earlier bound.
                None => false,
            }
        }
    };
    if replace {
        *slot = Some(number);
    }
}

/// The bound as the column's type takes it, or `None` when it cannot.
fn number_bound(column: &Column, bound: &Bound) -> Option<NumberBound> {
    let literal = &bound.literal;
    let exact = match column.base_type() {
        BaseType::Integer => {
            let (min, max) = BaseType::integer_bounds(&column.sqlx_type)?;
            let value = inclusive(literal.integer()?, bound)?;
            return (min..=max).contains(&value).then(|| NumberBound {
                value: value.to_string(),
                exclusive: false,
            });
        }
        BaseType::Float => literal.fits_binary(BaseType::float_mantissa_digits(&column.sqlx_type)?),
        BaseType::Decimal => {
            if !literal.fits_rust_decimal() {
                return None;
            }
            true
        }
        _ => return None,
    };
    if bound.strict && !exact {
        return None;
    }
    Some(NumberBound {
        value: literal.to_string(),
        exclusive: bound.strict,
    })
}

/// An integer limit as an inclusive one: `> n` is `>= n + 1`.
fn inclusive(limit: i128, bound: &Bound) -> Option<i128> {
    match (bound.strict, bound.lower) {
        (false, _) => Some(limit),
        (true, true) => limit.checked_add(1),
        (true, false) => limit.checked_sub(1),
    }
}

/// A number literal, exactly: `mantissa / 10^scale`, with no trailing zero
/// after the point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Decimal {
    mantissa: i128,
    scale: u32,
}

impl Decimal {
    /// Reads `12`, `0.50`, or `.5`. Exponents and literals too long for an
    /// `i128` mantissa are `None`.
    fn parse(text: &str) -> Option<Self> {
        let (negative, text) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
        if whole.is_empty() && fraction.is_empty() {
            return None;
        }
        let fraction = fraction.trim_end_matches('0');
        let mut mantissa: i128 = 0;
        for character in whole.chars().chain(fraction.chars()) {
            let digit = character.to_digit(10)?;
            mantissa = mantissa.checked_mul(10)?.checked_add(i128::from(digit))?;
        }
        let decimal = Self {
            mantissa,
            scale: u32::try_from(fraction.len()).ok()?,
        };
        if negative {
            decimal.negated()
        } else {
            Some(decimal)
        }
    }

    fn negated(self) -> Option<Self> {
        Some(Self {
            mantissa: self.mantissa.checked_neg()?,
            ..self
        })
    }

    /// The value, when it is a whole number.
    fn integer(&self) -> Option<i128> {
        (self.scale == 0).then_some(self.mantissa)
    }

    /// Orders two literals, or `None` when they are too long to bring to
    /// one scale.
    fn compare(&self, other: &Self) -> Option<Ordering> {
        let scale = self.scale.max(other.scale);
        let left = self
            .mantissa
            .checked_mul(10i128.checked_pow(scale - self.scale)?)?;
        let right = other
            .mantissa
            .checked_mul(10i128.checked_pow(scale - other.scale)?)?;
        Some(left.cmp(&right))
    }

    /// Whether a binary float with `digits` bits of mantissa holds the value
    /// exactly: `m / 10^s` is `m / 5^s / 2^s`, so `5^s` must divide `m`,
    /// and what is left, without its factors of two, must fit the mantissa.
    /// Literals are far inside the exponent range of either float.
    fn fits_binary(&self, digits: u32) -> bool {
        let Some(fives) = 5u128.checked_pow(self.scale) else {
            return false;
        };
        let magnitude = self.mantissa.unsigned_abs();
        if !magnitude.is_multiple_of(fives) {
            return false;
        }
        let odd = magnitude / fives;
        let odd = if odd == 0 {
            0
        } else {
            odd >> odd.trailing_zeros()
        };
        odd < 1u128 << digits
    }

    /// Whether `rust_decimal::Decimal` holds the value: a mantissa of at
    /// most 96 bits and at most 28 digits after the point.
    fn fits_rust_decimal(&self) -> bool {
        self.scale <= 28 && self.mantissa.unsigned_abs() < 1u128 << 96
    }
}

impl std::fmt::Display for Decimal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sign = if self.mantissa < 0 { "-" } else { "" };
        let digits = self.mantissa.unsigned_abs().to_string();
        let scale = self.scale as usize;
        if scale == 0 {
            return write!(formatter, "{sign}{digits}");
        }
        let digits = format!("{digits:0>width$}", width = scale + 1);
        let (whole, fraction) = digits.split_at(digits.len() - scale);
        write!(formatter, "{sign}{whole}.{fraction}")
    }
}
