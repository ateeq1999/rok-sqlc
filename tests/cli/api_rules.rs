//! `[api.rules]`: string rules by table and column, read from the config
//! file and the JSON payload, layered by table and then by column, and
//! checked against the schema and VineJS's rule names.

use super::api::{api_project, stderr};
use super::*;

/// Rules as strings and inline tables generate; the payload's rules for a
/// column replace the file's, and other columns keep theirs.
#[test]
fn rules_read_from_the_file_and_layer_by_column() -> Result<()> {
    let dir = api_project(
        "[api]
output = \"src/features\"

[api.rules.users]
email = [\"email\", \"trim\", \"toLowerCase\"]
password_hash = [{ regex = \"^[a-z0-9$./]+$\" }, \"minLength(8)\", { notIn = [\"a,b\", \"c\"] }]
",
    )?;
    let run = |args: &[&str]| run_binary_in(dir.path(), args);
    assert_success(&run(&["g"]));
    let dto = fs::read_to_string(dir.path().join("src/features/users/dto.rs"))?;
    assert!(dto.contains("fn email_transforms(value: &str)"), "{dto}");
    assert!(
        dto.contains("const PATTERN: &str = \"^[a-z0-9$./]+$\";"),
        "{dto}"
    );
    assert!(
        dto.contains("const VALUES: &[&str] = &[\"a,b\", \"c\"];"),
        "{dto}"
    );

    // The payload replaces `email`'s rules and keeps `password_hash`'s.
    assert_success(&run(&[
        "g",
        "-j",
        r#"{"api": {"rules": {"users": {"email": ["url"]}}}}"#,
    ]));
    let dto = fs::read_to_string(dir.path().join("src/features/users/dto.rs"))?;
    assert!(!dto.contains("fn email_transforms("), "{dto}");
    assert!(dto.contains("code = \"url\""), "{dto}");
    assert!(dto.contains("fn password_hash_min_length("), "{dto}");

    // The payload checks like the file.
    let failed = run(&[
        "g",
        "-j",
        r#"{"api": {"rules": {"users": {"email": ["emial"]}}}}"#,
    ]);
    assert!(
        stderr(&failed).contains(
            "`api.rules.users.email` in the configuration has `emial`, which is no string rule"
        ),
        "{}",
        stderr(&failed)
    );
    Ok(())
}

/// Every way a rule can be wrong fails the run with what is wrong, and
/// writes nothing.
#[test]
fn bad_rules_fail_clearly() -> Result<()> {
    let cases = [
        (
            "[api.rules.comments]\nnote = [\"email\"]",
            "`api.rules.comments` in the configuration names no generated table of the schema",
        ),
        (
            "[api.rules.users]\nnope = [\"email\"]",
            "`api.rules.users` in the configuration names `nope`, which is not a column of `users`",
        ),
        (
            "[api.rules.users]\nage = [\"email\"]",
            "`api.rules.users` in the configuration names `age`, which is not a text column, so string rules can't check it",
        ),
        (
            "[api.rules.audit_log]\nnote = [\"trim\"]\n[conventions]\nmanaged_columns = [\"note\"]",
            "`api.rules.audit_log` in the configuration names `note`, a managed column, which generated code never writes",
        ),
        (
            "[api.rules.users]\nemail = [\"emial\"]",
            "`api.rules.users.email` in the configuration has `emial`, which is no string rule; the rules are `email`, `url`, `uuid`, `ulid`, `ipAddress`, `mobile`, `alpha`, `alphaNumeric`, `ascii`, `regex`, `in`, `notIn`, `startsWith`, `endsWith`, `minLength`, `maxLength`, `fixedLength`, `hexCode`, `iban`, `jwt`, `postalCode`, `creditCard` and `coordinates`, and the transforms `trim`, `toLowerCase`, `toUpperCase` and `normalizeEmail`",
        ),
        (
            "[api.rules.users]\nemail = [\"minLength(three)\"]",
            "`api.rules.users.email` in the configuration has `minLength(three)`, but `minLength` takes a number of characters, such as `minLength(3)`",
        ),
        (
            "[api.rules.users]\nemail = [\"email\", \"email\"]",
            "`api.rules.users.email` in the configuration has `email` twice",
        ),
        (
            "[api.rules.users]\nemail = [\"sameAs(password_hash)\"]",
            "`api.rules.users.email` in the configuration has `sameAs`, which rok-sqlc leaves out: cross-field rules belong in hand-written request types",
        ),
        (
            "[api.rules.users]\nemail = [\"activeUrl\"]",
            "`api.rules.users.email` in the configuration has `activeUrl`, which rok-sqlc leaves out: it looks up DNS records while validating",
        ),
        (
            "[api.rules.users]\nemail = [\"mobile(xx-XX)\"]",
            "`api.rules.users.email` in the configuration has `mobile(xx-XX)`, but `mobile` takes one or more locales, such as `mobile(ar-SA)`, and `strictMode`; the locales are `ar-AE`",
        ),
        (
            "[api.rules.users]\nemail = [{ regex = \"(\" }]",
            "`api.rules.users.email` in the configuration has the pattern `(`, which is no regular expression",
        ),
        (
            "[api.rules.users]\nemail = [{ pattern = \"x\" }]",
            "an inline rule takes one key: `regex`, `in`, `notIn`, `startsWith` or `endsWith`",
        ),
        (
            "[api.rules.users]\nemail = [{ in = [] }]",
            "`api.rules.users.email` in the configuration has an inline `in` or `notIn` with no values",
        ),
    ];
    for (entry, expected) in cases {
        let dir = api_project(&format!("[api]\noutput = \"src/features\"\n{entry}\n"))?;

        let output = run_binary_in(dir.path(), &["g"]);

        let stderr = stderr(&output);
        assert!(!output.status.success(), "{entry} succeeded");
        assert!(stderr.contains(expected), "{entry}:\n{stderr}");
        assert!(!dir.path().join("out").exists(), "{entry} wrote files");
    }
    Ok(())
}
