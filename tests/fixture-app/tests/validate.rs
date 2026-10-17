//! Validation rules on the create inputs and update patches generated with
//! `derive = ["serde", "validate"]` (`src/validated_db/`, from
//! `validated_schema.sql`). No database needed.

use rok_sqlc_fixture_app::validated_db::players::models::{NewPlayer, UpdatePlayer};
use rok_sqlc_fixture_app::validated_db::teams::models::{NewTeam, TeamId, UpdateTeam};
use rust_decimal::Decimal;
use validator::{Validate, ValidationErrors};

fn player() -> NewPlayer {
    NewPlayer {
        team_code: Some(TeamId("NORTH".to_owned())),
        handle: "ace".to_owned(),
        bio: None,
        jersey: 7,
        rating: Some(9.5),
        salary: Some(Decimal::new(125_000, 2)),
        nickname: None,
        joined_year: None,
    }
}

/// Each failing field with its error's code and message.
fn failures(errors: &ValidationErrors) -> Vec<(String, String, String)> {
    let mut failures: Vec<(String, String, String)> = errors
        .field_errors()
        .iter()
        .flat_map(|(field, errors)| {
            errors.iter().map(move |error| {
                (
                    field.to_string(),
                    error.code.to_string(),
                    error.message.as_deref().unwrap_or_default().to_owned(),
                )
            })
        })
        .collect();
    failures.sort();
    failures
}

fn failure(field: &str, code: &str, message: &str) -> (String, String, String) {
    (field.to_owned(), code.to_owned(), message.to_owned())
}

#[test]
fn a_valid_input_passes() {
    assert_eq!(player().validate(), Ok(()));
    // Lengths count characters, not bytes, as PostgreSQL counts them.
    let wide = NewPlayer {
        handle: "é".repeat(20),
        nickname: Some(Some("ü".repeat(12))),
        ..player()
    };
    assert_eq!(wide.validate(), Ok(()));
}

#[test]
fn an_input_breaking_rules_fails_field_by_field() {
    let input = NewPlayer {
        team_code: Some(TeamId("NORTHWEST".to_owned())),
        handle: "x".repeat(21),
        bio: Some("y".repeat(201)),
        jersey: 100,
        rating: Some(0.0),
        salary: Some(Decimal::new(-1, 2)),
        nickname: Some(Some("z".repeat(13))),
        // Only the database checks this one.
        joined_year: Some(1800),
    };
    let errors = input.validate().expect_err("every rule is broken");
    assert_eq!(
        failures(&errors),
        [
            failure("bio", "length", "bio must be at most 200 characters"),
            failure("handle", "length", "handle must be at most 20 characters"),
            failure("jersey", "range", "jersey must be between 0 and 99"),
            failure(
                "nickname",
                "length",
                "nickname must be at most 12 characters"
            ),
            failure(
                "rating",
                "range",
                "rating must be greater than 0 and at most 10"
            ),
            failure("salary", "range", "salary must be at least 0"),
            failure(
                "team_code",
                "length",
                "team_code must be at most 8 characters"
            ),
        ]
    );
}

#[test]
fn absent_and_null_input_fields_pass() {
    let input = NewPlayer {
        team_code: None,
        rating: None,
        salary: None,
        nickname: Some(None),
        ..player()
    };
    assert_eq!(input.validate(), Ok(()));
}

#[test]
fn a_typed_text_key_keeps_its_length_rule() {
    let team = NewTeam {
        code: TeamId("NORTHWEST".to_owned()),
        name: "N".to_owned(),
    };
    let errors = team.validate().expect_err("both rules are broken");
    assert_eq!(
        failures(&errors),
        [
            failure("code", "length", "code must be at most 8 characters"),
            failure("name", "length", "name must be between 2 and 40 characters"),
        ]
    );
}

#[test]
fn a_patch_checks_only_the_values_it_sets() {
    assert_eq!(UpdatePlayer::default().validate(), Ok(()));
    assert_eq!(UpdateTeam::default().validate(), Ok(()));
    // `Some(None)` sets NULL, which no rule constrains.
    let nulls = UpdatePlayer {
        bio: Some(None),
        rating: Some(None),
        nickname: Some(None),
        team_code: Some(None),
        ..UpdatePlayer::default()
    };
    assert_eq!(nulls.validate(), Ok(()));

    let patch = UpdatePlayer {
        handle: Some("ok".to_owned()),
        bio: Some(Some("y".repeat(201))),
        jersey: Some(-1),
        salary: Some(Decimal::new(1, 2)),
        ..UpdatePlayer::default()
    };
    let errors = patch.validate().expect_err("two rules are broken");
    assert_eq!(
        failures(&errors),
        [
            failure("bio", "length", "bio must be at most 200 characters"),
            failure("jersey", "range", "jersey must be between 0 and 99"),
        ]
    );

    let team = UpdateTeam {
        name: Some("Northern".to_owned()),
    };
    assert_eq!(team.validate(), Ok(()));
}
