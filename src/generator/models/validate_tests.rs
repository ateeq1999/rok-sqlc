//! The validation rules of a create input and an update patch (ADR 0015).

use super::super::test_support::{column, nullable, table};
use super::*;
use crate::schema::{LengthRange, NumberBound, TypedId, ValueRange};

/// `validate` derives `validator::Validate` for the create input and the
/// update patch, and for nothing else (ADR 0015).
#[test]
fn validate_derives_only_inputs_and_patches() {
    let users = table(
        vec![column("id", "i64"), column("email", "String")],
        &["id"],
    );
    let rendered = super::render(
        &users,
        DeriveOptions {
            serde: true,
            validate: true,
            openapi: false,
        },
    );
    let derive = "serde::Serialize, serde::Deserialize, validator::Validate)]\n";
    assert!(
        rendered.contains(&format!("{derive}pub struct NewUser {{")),
        "{rendered}"
    );
    assert!(
            rendered.contains(
                "serde::Deserialize, validator::Validate,\n)]\n#[serde(default)]\npub struct UpdateUser {"
            ),
            "{rendered}"
        );
    assert_eq!(rendered.matches("validator::Validate").count(), 2);
    assert!(
        !super::render(
            &users,
            DeriveOptions {
                serde: true,
                validate: false,
                openapi: false,
            },
        )
        .contains("validat")
    );
}

/// A text column's declared length and length checks become a `length`
/// rule, counted in characters, on the input and the patch alike.
#[test]
fn validate_puts_length_rules_on_text_fields() {
    let lengths = |declared: Option<u32>, min: Option<u32>, max: Option<u32>| {
        move |mut column: Column| {
            column.declared_length = declared;
            if min.is_some() || max.is_some() {
                column.length_range = Some(LengthRange { min, max });
            }
            column
        }
    };
    let users = table(
        vec![
            column("id", "i64"),
            lengths(Some(80), None, None)(column("name", "String")),
            lengths(Some(80), Some(1), Some(100))(column("title", "String")),
            lengths(None, Some(3), None)(nullable("code", "String")),
            lengths(Some(1), None, None)(column("grade", "String")),
            lengths(Some(3), Some(3), None)(column("iso", "String")),
            column("body", "String"),
        ],
        &["id"],
    );
    let rendered = super::render(
        &users,
        DeriveOptions {
            serde: true,
            validate: true,
            openapi: false,
        },
    );
    let (input, patch) = rendered
        .split_once("pub struct UpdateUser {")
        .expect("a patch");
    let rules = [
        (
            "name",
            "#[validate(length(max = 80, message = \"name must be at most 80 characters\"))]",
        ),
        (
            "title",
            "#[validate(length(\n        min = 1,\n        max = 80,\n        message = \"title must be between 1 and 80 characters\"\n    ))]",
        ),
        (
            "code",
            "#[validate(length(min = 3, message = \"code must be at least 3 characters\"))]",
        ),
        (
            "grade",
            "#[validate(length(max = 1, message = \"grade must be at most 1 character\"))]",
        ),
        (
            "iso",
            "#[validate(length(equal = 3, message = \"iso must be exactly 3 characters\"))]",
        ),
    ];
    for (field, rule) in rules {
        for part in [input, patch] {
            let expected = format!("    {rule}\n    pub {field}: ");
            assert!(part.contains(&expected), "{expected}\n{part}");
        }
    }
    assert_eq!(rendered.matches("#[validate(").count(), 10, "{rendered}");
    assert!(
        !super::render(
            &users,
            DeriveOptions {
                serde: true,
                validate: false,
                openapi: false,
            },
        )
        .contains("validat")
    );
}

/// A numeric column's range checks become a `range` rule; a decimal one,
/// which `validator` cannot compare, a `custom` rule with the stock
/// `range` code, checked by a function in `models.rs`.
#[test]
fn validate_puts_range_rules_on_numeric_fields() {
    let bound = |value: &str, exclusive: bool| NumberBound {
        value: value.to_owned(),
        exclusive,
    };
    let ranged = |min: Option<NumberBound>, max: Option<NumberBound>| {
        move |mut column: Column| {
            column.value_range = Some(ValueRange { min, max });
            column
        }
    };
    let mut id = column("id", "i64");
    id.typed_id = Some(TypedId {
        module: "users".to_owned(),
        name: "UserId".to_owned(),
    });
    let users = table(
        vec![
            ranged(Some(bound("1", false)), None)(id),
            ranged(Some(bound("0", false)), None)(column("qty", "i32")),
            ranged(Some(bound("1", false)), Some(bound("5", false)))(nullable("rating", "i16")),
            ranged(Some(bound("0", true)), Some(bound("1", false)))(column("score", "f64")),
            ranged(None, Some(bound("-0.5", true)))(column("ratio", "f32")),
            ranged(Some(bound("0", false)), Some(bound("99.95", true)))(column(
                "price",
                "rust_decimal::Decimal",
            )),
        ],
        &["id"],
    );
    let rendered = super::render(
        &users,
        DeriveOptions {
            serde: true,
            validate: true,
            openapi: false,
        },
    );
    let (input, patch) = rendered
        .split_once("pub struct UpdateUser {")
        .expect("a patch");
    let rules = [
        (
            "qty",
            "#[validate(range(min = 0, message = \"qty must be at least 0\"))]",
        ),
        (
            "rating",
            "#[validate(range(min = 1, max = 5, message = \"rating must be between 1 and 5\"))]",
        ),
        (
            "score",
            "#[validate(range(\n        exclusive_min = 0.0,\n        max = 1.0,\n        message = \"score must be greater than 0 and at most 1\"\n    ))]",
        ),
        (
            "ratio",
            "#[validate(range(exclusive_max = -0.5, message = \"ratio must be less than -0.5\"))]",
        ),
        (
            "price",
            "#[validate(custom(\n        function = \"price_in_range\",\n        code = \"range\",\n        message = \"price must be at least 0 and less than 99.95\"\n    ))]",
        ),
    ];
    for (field, rule) in rules {
        for part in [input, patch] {
            let expected = format!("    {rule}\n    pub {field}: ");
            assert!(part.contains(&expected), "{expected}\n{part}");
        }
    }
    // The typed id field takes its range too, through `ValidateRange`.
    let id_rule =
        "    #[validate(range(min = 1, message = \"id must be at least 1\"))]\n    pub id: UserId,";
    assert!(input.contains(id_rule), "{input}");
    assert!(
        rendered.contains("impl validator::ValidateRange<i64> for UserId {"),
        "{rendered}"
    );
    assert_eq!(rendered.matches("#[validate(").count(), 11, "{rendered}");
    assert!(
        rendered.ends_with(
            "/// The range rule of the `price` column.
fn price_in_range(value: &rust_decimal::Decimal) -> Result<(), validator::ValidationError> {
    let min = rust_decimal::Decimal::from_i128_with_scale(0, 0);
    let exclusive_max = rust_decimal::Decimal::from_i128_with_scale(9995, 2);
    if *value < min || *value >= exclusive_max {
        let mut error = validator::ValidationError::new(\"range\");
        error.add_param(\"min\".into(), &min);
        error.add_param(\"exclusive_max\".into(), &exclusive_max);
        return Err(error);
    }
    Ok(())
}
"
        ),
        "{rendered}"
    );
    assert!(
        !super::render(
            &users,
            DeriveOptions {
                serde: true,
                validate: false,
                openapi: false,
            },
        )
        .contains("validat")
    );
}

/// A typed id over text measures the text inside, so a key column, or a
/// foreign key using the id, keeps its length rule.
#[test]
fn validate_makes_text_typed_ids_measurable() {
    let typed = |mut column: Column, name: &str, module: &str| {
        column.typed_id = Some(TypedId {
            module: module.to_owned(),
            name: name.to_owned(),
        });
        column.declared_length = Some(12);
        column
    };
    let users = table(
        vec![
            typed(column("code", "String"), "UserId", "users"),
            typed(nullable("team_code", "String"), "TeamId", "teams"),
        ],
        &["code"],
    );
    let rendered = super::render(
        &users,
        DeriveOptions {
            serde: true,
            validate: true,
            openapi: false,
        },
    );
    assert!(
        rendered.contains(
            "        std::fmt::Display::fmt(&self.0, formatter)
    }
}

impl validator::ValidateLength<u64> for UserId {
    fn length(&self) -> Option<u64> {
        validator::ValidateLength::length(&self.0)
    }
}
"
        ),
        "{rendered}"
    );
    assert_eq!(
        rendered.matches("impl validator::").count(),
        1,
        "{rendered}"
    );
    for field in ["code: UserId", "team_code: Option<TeamId>"] {
        assert!(
            rendered.contains(&format!(
                "message = \"{} must be at most 12 characters\"))]\n    pub {field}",
                field.split(':').next().unwrap()
            )),
            "{field}\n{rendered}"
        );
    }
    assert!(
        !super::render(
            &users,
            DeriveOptions {
                serde: true,
                validate: false,
                openapi: false,
            },
        )
        .contains("validat")
    );

    // A typed id over an integer compares the number inside instead.
    let numbered = table(vec![typed(column("id", "i64"), "UserId", "users")], &["id"]);
    let rendered = super::render(
        &numbered,
        DeriveOptions {
            serde: true,
            validate: true,
            openapi: false,
        },
    );
    assert!(!rendered.contains("ValidateLength"), "{rendered}");
    assert!(
        rendered.contains(
            "impl validator::ValidateRange<i64> for UserId {
    fn greater_than(&self, max: i64) -> Option<bool> {
        validator::ValidateRange::greater_than(&self.0, max)
    }

    fn less_than(&self, min: i64) -> Option<bool> {
        validator::ValidateRange::less_than(&self.0, min)
    }
}
"
        ),
        "{rendered}"
    );
}
