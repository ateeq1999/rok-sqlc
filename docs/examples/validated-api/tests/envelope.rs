//! The rejections the README shows, as `db::http` answers them on either
//! framework.

use rok_sqlc_validated_api::db::http::{BodyRejection, validated};
use rok_sqlc_validated_api::db::members::models::NewMember;
use serde_json::{Value, json};

/// `body` read as both extractors read it: serde's error classified by
/// `BodyRejection::from_serde`, then the validation rules.
fn read(body: &str) -> Result<NewMember, BodyRejection> {
    let input = serde_json::from_str(body).map_err(|error| BodyRejection::from_serde(&error))?;
    validated(input)
}

/// The response body rejecting `body`.
fn rejection(body: &str) -> Value {
    let rejection = read(body).expect_err("a bad body");
    serde_json::to_value(rejection.body()).expect("the envelope serializes")
}

#[test]
fn a_body_breaking_rules_gets_422_with_details() {
    let body = r#"{"name": "", "handle": "ada", "age": 7}"#;
    let expected: Value = serde_json::from_str(
        r#"
{
  "success": false,
  "error": {
    "message": "the request body failed validation",
    "code": "VALIDATION_FAILED",
    "details": {
      "name": [
        {
          "code": "length",
          "message": "name must be between 1 and 80 characters",
          "params": { "min": 1, "max": 80, "value": "" }
        }
      ],
      "age": [
        {
          "code": "range",
          "message": "age must be between 13 and 130",
          "params": { "min": 13, "max": 130, "value": 7 }
        }
      ]
    }
  }
}
"#,
    )
    .expect("valid JSON");
    assert_eq!(rejection(body), expected);
}

#[test]
fn a_body_that_does_not_fit_gets_422_without_details() {
    let body = r#"{"name": "Ada"}"#;
    assert_eq!(
        rejection(body),
        json!({
            "success": false,
            "error": {
                "message": "missing field `handle` at line 1 column 15",
                "code": "VALIDATION_FAILED"
            }
        })
    );
}

#[test]
fn a_body_that_is_not_json_gets_400() {
    let body = rejection(r#"{"name": "#);
    assert_eq!(body["error"]["code"], "MALFORMED_BODY");
}

#[test]
fn a_valid_body_passes() {
    let body = r#"{"name": "Ada", "handle": "ada", "age": 36, "balance": "12.50"}"#;
    let input = read(body).expect("a valid body");
    assert_eq!(input.handle, "ada");
}
