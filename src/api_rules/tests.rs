//! The tests of parsing and checking `[api.rules]`.

use super::*;

fn text(text: &str) -> Result<Rule, String> {
    parse(&ApiRule::from(text))
}

#[test]
fn rules_parse_with_their_arguments() {
    assert_eq!(text("email"), Ok(Rule::Email));
    assert_eq!(text("minLength(3)"), Ok(Rule::MinLength(3)));
    assert_eq!(text("uuid(4, 7)"), Ok(Rule::Uuid(vec![4, 7])));
    assert_eq!(text("ipAddress(6)"), Ok(Rule::IpAddress(Some(6))));
    assert_eq!(
        text("alphaNumeric(allowDashes, allowSpaces)"),
        Ok(Rule::AlphaNumeric(Extras {
            spaces: true,
            dashes: true,
            underscores: false
        }))
    );
    assert_eq!(
        text("in(SA, AE, EG)"),
        Ok(Rule::In(vec!["SA".into(), "AE".into(), "EG".into()]))
    );
    assert_eq!(
        text("mobile(ar-SA, strictMode)"),
        Ok(Rule::Mobile {
            locales: vec!["ar-SA".into()],
            strict: true
        })
    );
    assert_eq!(
        parse(&ApiRule::In(vec!["a,b".into(), "c".into()])),
        Ok(Rule::In(vec!["a,b".into(), "c".into()]))
    );
    assert_eq!(text("creditCard"), Ok(Rule::CreditCard(Vec::new())));
    // No locale: any locale rok-sqlc carries.
    assert_eq!(
        text("mobile"),
        Ok(Rule::Mobile {
            locales: Vec::new(),
            strict: false
        })
    );
}

#[test]
fn bad_rules_say_what_they_take() {
    let error = |rule: &str| text(rule).expect_err(rule);
    assert_eq!(
        error("minLength(x)"),
        "`minLength(x)`, but `minLength` takes a number of characters, such as `minLength(3)`"
    );
    assert_eq!(
        error("email(x)"),
        "`email(x)`, but `email` takes no arguments"
    );
    assert_eq!(
        error("uuid(9)"),
        "`uuid(9)`, but `uuid` takes UUID versions from 1 to 8, such as `uuid(4, 7)`"
    );
    assert!(
        error("nope").starts_with("`nope`, which is no string rule; the rules are `email`, `url`")
    );
    assert!(error("mobile(xx-XX)").contains("the locales are `ar-AE`"));
    assert!(error("postalCode(IN)").contains("the countries are `CA`"));
    assert!(
        error("sameAs(password)")
            .contains("cross-field rules belong in hand-written request types")
    );
    assert!(error("regex(^a$)").contains("`{ regex = \"^[a-z0-9-]+$\" }`"));
    assert!(error("minLength(3").contains("no closing `)`"));
    let pattern = parse(&ApiRule::Regex("(".into())).expect_err("an unclosed group");
    assert!(
        pattern.starts_with("the pattern `(`, which is no regular expression"),
        "{pattern}"
    );
}

#[test]
fn display_writes_the_rule_back() {
    for rule in [
        "minLength(3)",
        "in(SA, AE)",
        "alpha(allowSpaces)",
        "uuid",
        "mobile(ar-SA, strictMode)",
        "mobile",
    ] {
        assert_eq!(text(rule).map(|rule| rule.display()), Ok(rule.to_owned()));
    }
}
