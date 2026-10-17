//! Declared lengths and simple `CHECK` rules kept on columns (ADR 0015).

use super::*;
use crate::schema::{Column, LengthRange};

fn column<'a>(table: &'a Table, name: &str) -> &'a Column {
    table
        .columns
        .iter()
        .find(|column| column.sql_name == name)
        .unwrap_or_else(|| panic!("no column `{name}`"))
}

/// The declared length of each column, in column order.
fn lengths(table: &Table) -> Vec<(&str, Option<u32>)> {
    table
        .columns
        .iter()
        .map(|column| (column.sql_name.as_str(), column.declared_length))
        .collect()
}

#[test]
fn character_types_keep_their_declared_length() {
    let table = one_table(
        "CREATE TABLE people (
             id BIGSERIAL PRIMARY KEY,
             name VARCHAR(80) NOT NULL,
             nick CHARACTER VARYING(12),
             code CHAR(3),
             grade CHARACTER(1),
             flag CHAR,
             note VARCHAR,
             body TEXT
         );",
    );
    assert_eq!(
        lengths(&table),
        [
            ("id", None),
            ("name", Some(80)),
            ("nick", Some(12)),
            ("code", Some(3)),
            ("grade", Some(1)),
            ("flag", Some(1)),
            ("note", None),
            ("body", None),
        ]
    );
}

/// A column added later keeps its length; a type change replaces it or,
/// to a type without one, clears it.
#[test]
fn lengths_follow_added_columns_and_type_changes() {
    let parsed = parse_files(&[
        (
            "0001.up.sql",
            "CREATE TABLE people (id BIGSERIAL PRIMARY KEY, name VARCHAR(80), code CHAR(3), note TEXT);",
        ),
        (
            "0002.up.sql",
            "ALTER TABLE people ADD COLUMN email VARCHAR(255) NOT NULL;
             ALTER TABLE people ALTER COLUMN name TYPE VARCHAR(120);
             ALTER TABLE people ALTER COLUMN code TYPE TEXT;
             ALTER TABLE people ALTER COLUMN note SET DATA TYPE CHARACTER VARYING(500);",
        ),
    ]);
    assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
    assert_eq!(
        lengths(&parsed.schema.tables[0]),
        [
            ("id", None),
            ("name", Some(120)),
            ("code", None),
            ("note", Some(500)),
            ("email", Some(255)),
        ]
    );
}

/// Each column's value range, as `"min, max"` with `>=`/`>` and `<=`/`<`
/// marking inclusive and exclusive bounds and `-` for a missing one; `None`
/// for a column without a range.
fn ranges(table: &Table) -> Vec<(&str, Option<String>)> {
    table
        .columns
        .iter()
        .map(|column| {
            let range = column.value_range.as_ref().map(|range| {
                let min = range.min.as_ref().map_or("-".to_owned(), |bound| {
                    let op = if bound.exclusive { ">" } else { ">=" };
                    format!("{op}{}", bound.value)
                });
                let max = range.max.as_ref().map_or("-".to_owned(), |bound| {
                    let op = if bound.exclusive { "<" } else { "<=" };
                    format!("{op}{}", bound.value)
                });
                format!("{min}, {max}")
            });
            (column.sql_name.as_str(), range)
        })
        .collect()
}

fn range(text: &str) -> Option<String> {
    Some(text.to_owned())
}

/// Comparisons with a literal on either side, `BETWEEN`, and an `AND` of
/// them give an integer column a range; strict bounds become inclusive.
#[test]
fn comparisons_give_integer_columns_an_inclusive_range() {
    let table = one_table(
        "CREATE TABLE stock (
             id BIGSERIAL PRIMARY KEY,
             at_least INTEGER CHECK (at_least >= 0),
             above INTEGER CHECK (above > 0),
             at_most SMALLINT CHECK (at_most <= 10),
             below BIGINT CHECK ((below < -5)),
             left_side INTEGER CHECK (1 < left_side),
             left_max INTEGER CHECK (100 >= left_max),
             between_them INTEGER CHECK (between_them BETWEEN -3 AND 7),
             both_ends INTEGER CHECK (both_ends > 0 AND both_ends <= 99),
             table_level INTEGER,
             named INTEGER,
             plain INTEGER,
             CHECK (table_level >= 18),
             CONSTRAINT named_positive CHECK (0 < named AND named < 1000)
         );",
    );
    assert_eq!(
        ranges(&table),
        [
            ("id", None),
            ("at_least", range(">=0, -")),
            ("above", range(">=1, -")),
            ("at_most", range("-, <=10")),
            ("below", range("-, <=-6")),
            ("left_side", range(">=2, -")),
            ("left_max", range("-, <=100")),
            ("between_them", range(">=-3, <=7")),
            ("both_ends", range(">=1, <=99")),
            ("table_level", range(">=18, -")),
            ("named", range(">=1, <=999")),
            ("plain", None),
        ]
    );
}

/// On floats and `NUMERIC`, inclusive bounds are kept as written and a
/// strict bound stays strict, kept only when the type holds its literal
/// exactly. Bounds an integer column cannot take are dropped.
#[test]
fn strict_bounds_on_other_numbers_are_kept_only_when_exact() {
    let table = one_table(
        "CREATE TABLE prices (
             id BIGSERIAL PRIMARY KEY,
             amount NUMERIC(12, 2) CHECK (amount > 0 AND amount <= 99999.990),
             ratio DOUBLE PRECISION CHECK (ratio > 0.5 AND ratio < 0.1),
             share REAL CHECK (share >= 0.1 AND share < 1),
             tiny SMALLINT CHECK (tiny < 100000 AND tiny >= -5),
             whole INTEGER CHECK (whole > 0.5 AND whole <= 10.0),
             huge NUMERIC CHECK (huge < 123456789012345678901234567890),
             label TEXT CHECK (label > 'a'),
             born DATE CHECK (born > 0)
         );",
    );
    assert_eq!(
        ranges(&table),
        [
            ("id", None),
            ("amount", range(">0, <=99999.99")),
            ("ratio", range(">0.5, -")),
            ("share", range(">=0.1, <1")),
            ("tiny", range(">=-5, -")),
            ("whole", range("-, <=10")),
            ("huge", None),
            ("label", None),
            ("born", None),
        ]
    );
}

/// The tightest of several bounds on one end wins, across checks.
#[test]
fn several_checks_on_one_column_keep_the_tightest_bounds() {
    let table = one_table(
        "CREATE TABLE stock (
             id BIGSERIAL PRIMARY KEY,
             count INTEGER CHECK (count >= 0) CHECK (count > 5),
             level NUMERIC CHECK (level >= 1.5),
             CHECK (count BETWEEN 1 AND 50),
             CHECK (level > 1.5 AND level <= 3),
             CHECK (level < 2.25)
         );",
    );
    assert_eq!(
        ranges(&table),
        [
            ("id", None),
            ("count", range(">=6, <=50")),
            ("level", range(">1.5, <2.25")),
        ]
    );
}

/// Each column's length range.
fn length_ranges(table: &Table) -> Vec<(&str, Option<LengthRange>)> {
    table
        .columns
        .iter()
        .map(|column| (column.sql_name.as_str(), column.length_range))
        .collect()
}

/// `char_length`, `character_length`, and `length` compared with an
/// integer give a text column a length range, inclusive at both ends.
#[test]
fn length_functions_give_text_columns_a_length_range() {
    let table = one_table(
        "CREATE TABLE people (
             id BIGSERIAL PRIMARY KEY,
             name TEXT CHECK (char_length(name) > 0),
             nick VARCHAR(40) CHECK (CHARACTER_LENGTH(nick) <= 20),
             code TEXT CHECK (length(code) BETWEEN 2 AND 8),
             pin TEXT CHECK (4 <= length(pin) AND length(pin) < 9),
             any_length TEXT CHECK (length(any_length) >= 0),
             amount INTEGER CHECK (length(amount) > 1),
             CHECK (char_length(name) <= 100)
         );",
    );
    assert_eq!(
        length_ranges(&table),
        [
            ("id", None),
            ("name", lengths_between(Some(1), Some(100))),
            ("nick", lengths_between(None, Some(20))),
            ("code", lengths_between(Some(2), Some(8))),
            ("pin", lengths_between(Some(4), Some(8))),
            ("any_length", None),
            ("amount", None),
        ]
    );
    assert_eq!(column(&table, "nick").declared_length, Some(40));
}

/// Checks of any other shape keep giving no range, as before.
#[test]
fn other_checks_give_no_range() {
    let table = one_table(
        "CREATE TABLE stock (
             id BIGSERIAL PRIMARY KEY,
             a INTEGER,
             b INTEGER,
             name TEXT,
             CHECK (a < b),
             CHECK (a > 0 AND b > 0),
             CHECK (a > 0 OR a IS NULL),
             CHECK (b > 0 AND b <> 7),
             CHECK (NOT (b < 0)),
             CHECK (b NOT BETWEEN 1 AND 2),
             CHECK (b = 3),
             CHECK (abs(b) < 10),
             CHECK (octet_length(name) < 10),
             CHECK (upper(name) > 'A'),
             CHECK (length(name, 'UTF8') < 10),
             CHECK (stock.b > 1),
             CHECK (b > 1 + 1),
             CHECK (b::numeric > 1)
         );",
    );
    assert!(
        table
            .columns
            .iter()
            .all(|column| column.value_range.is_none() && column.length_range.is_none())
    );
}

/// Checks added with `ADD CONSTRAINT` or on an added column narrow ranges
/// like those in `CREATE TABLE`; a renamed column keeps its ranges, and a
/// dropped column takes them with it.
#[test]
fn ranges_follow_added_constraints_columns_and_renames() {
    let parsed = parse_files(&[
        (
            "0001.up.sql",
            "CREATE TABLE stock (
                 id BIGSERIAL PRIMARY KEY,
                 count INTEGER CHECK (count >= 0),
                 name TEXT,
                 doomed INTEGER CHECK (doomed > 0)
             );",
        ),
        (
            "0002.up.sql",
            "ALTER TABLE stock ADD CONSTRAINT count_capped CHECK (count < 500);
             ALTER TABLE stock ADD CHECK (char_length(name) BETWEEN 1 AND 60);
             ALTER TABLE stock ADD COLUMN price NUMERIC(8, 2) CHECK (price > 0);
             ALTER TABLE stock RENAME COLUMN count TO quantity;
             ALTER TABLE stock DROP COLUMN doomed;
             ALTER TABLE stock ADD COLUMN doomed INTEGER;",
        ),
    ]);
    assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
    let table = &parsed.schema.tables[0];
    assert_eq!(
        ranges(table),
        [
            ("id", None),
            ("quantity", range(">=0, <=499")),
            ("name", None),
            ("price", range(">0, -")),
            ("doomed", None),
        ]
    );
    assert_eq!(
        column(table, "name").length_range,
        Some(LengthRange {
            min: Some(1),
            max: Some(60)
        })
    );
}

/// A type change keeps the ranges while the Rust type stays the same, and
/// drops those the new type cannot take as they are.
#[test]
fn type_changes_keep_ranges_only_for_the_same_rust_type() {
    let parsed = parse_files(&[
        (
            "0001.up.sql",
            "CREATE TABLE stock (
                 id BIGSERIAL PRIMARY KEY,
                 count INTEGER CHECK (count > 0),
                 level INTEGER CHECK (level > 0),
                 name VARCHAR(10) CHECK (char_length(name) > 2),
                 code TEXT CHECK (char_length(code) > 2)
             );",
        ),
        (
            "0002.up.sql",
            "ALTER TABLE stock ALTER COLUMN count TYPE INT4;
             ALTER TABLE stock ALTER COLUMN level TYPE NUMERIC;
             ALTER TABLE stock ALTER COLUMN name TYPE TEXT;
             ALTER TABLE stock ALTER COLUMN code TYPE BYTEA USING code::bytea;",
        ),
    ]);
    assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
    let table = &parsed.schema.tables[0];
    assert_eq!(
        ranges(table),
        [
            ("id", None),
            ("count", range(">=1, -")),
            ("level", None),
            ("name", None),
            ("code", None),
        ]
    );
    assert!(column(table, "name").length_range.is_some());
    assert!(column(table, "code").length_range.is_none());
}

fn lengths_between(min: Option<u32>, max: Option<u32>) -> Option<LengthRange> {
    Some(LengthRange { min, max })
}
