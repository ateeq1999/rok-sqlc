//! String rules (`[api.rules]`, VineJS names) through the library entry
//! point: what each rule puts on the create and update bodies, and what the
//! OpenAPI document learns of it. The rules' behaviour is tested by running
//! them in `tests/api-app`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use rok_sqlc::{ApiConfig, ApiRule, Config, Derive, Extractor};
use tempfile::TempDir;

const SCHEMA: &str = "CREATE TABLE users (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL,
    nickname TEXT,
    handle VARCHAR(40) NOT NULL DEFAULT 'anonymous',
    age INT
);";

struct Project {
    dir: TempDir,
}

impl Project {
    fn new(schema: &str) -> Result<Self> {
        let dir = TempDir::new()?;
        fs::write(dir.path().join("schema.sql"), schema)?;
        Ok(Project { dir })
    }

    fn path(&self, path: &str) -> PathBuf {
        self.dir.path().join(path)
    }

    fn read(&self, path: &str) -> Result<String> {
        Ok(fs::read_to_string(self.path(path))?)
    }

    /// The configuration `rok-sqlc init` writes, with `rules` on `users`.
    fn config(&self, rules: &[(&str, &[&str])]) -> Config {
        let mut config = Config::default();
        config.schema = vec![self.path("schema.sql")];
        config.output = self.path("src/db");
        config.derive = vec![Derive::Serde, Derive::Validate];
        config.extractors = vec![Extractor::ActixWeb];
        let mut api = ApiConfig::new(self.path("src/features"));
        let columns: BTreeMap<String, Vec<ApiRule>> = rules
            .iter()
            .map(|(column, rules)| {
                let rules = rules.iter().map(|rule| ApiRule::from(*rule)).collect();
                ((*column).to_owned(), rules)
            })
            .collect();
        api.rules.insert("users".into(), columns);
        config.api = Some(api);
        config
    }

    /// Generates `config` and answers the users feature's `dto.rs`.
    fn dto(&self, config: &Config) -> Result<String> {
        rok_sqlc::generate(config)?;
        self.read("src/features/users/dto.rs")
    }
}

/// The struct `name` of a generated file: from its line to its closing
/// brace.
fn struct_block<'a>(text: &'a str, name: &str) -> &'a str {
    let start = text
        .find(&format!("pub struct {name} {{"))
        .unwrap_or_else(|| panic!("no {name} in\n{text}"));
    let end = start + text[start..].find("\n}\n").expect("the closing brace");
    &text[start..end]
}

/// `email` is validator's own rule, under VineJS's name; `trim` and
/// `toLowerCase` run while the body is read, so validation and the
/// database see the transformed value.
#[test]
fn email_with_transforms_on_both_bodies() -> Result<()> {
    let project = Project::new(SCHEMA)?;
    let config = project.config(&[("email", &["email", "trim", "toLowerCase"])]);
    let dto = project.dto(&config)?;

    let create = struct_block(&dto, "CreateUserDto");
    assert!(
        create.contains(
            "    /// The `email` column.
    ///
    /// Transforms: `trim`, `toLowerCase`.
    /// Rules: `email`.
    #[serde(deserialize_with = \"email_transformed\")]
    #[schema(format = \"email\")]
    #[validate(email(code = \"email\", message = \"email must be an email address\"))]
    pub email: String,"
        ),
        "{create}"
    );
    let update = struct_block(&dto, "UpdateUserDto");
    assert!(
        update.contains(
            "    #[serde(deserialize_with = \"email_transformed\")]
    #[schema(format = \"email\")]
    #[validate(email(code = \"email\", message = \"email must be an email address\"))]
    pub email: Option<String>,"
        ),
        "{update}"
    );
    assert!(
        dto.contains(
            "/// The `email` column's transforms: `trim`, then `toLowerCase`.
fn email_transforms(value: &str) -> String {
    let value = value.trim();
    value.to_lowercase()
}
"
        ),
        "{dto}"
    );
    assert!(
        dto.contains(
            "fn email_transformed<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Transformed<'de>,
{
    T::transformed(deserializer, email_transforms)
}
"
        ),
        "{dto}"
    );
    assert!(dto.contains("trait Transformed<'de>: Sized {"), "{dto}");
    Ok(())
}

/// A rule `validator` lacks is a `custom` rule calling a generated function
/// whose error code is the VineJS name; the OpenAPI document gets the
/// keywords utoipa can carry, and the rest in the field's description.
#[test]
fn other_rules_call_generated_functions() -> Result<()> {
    let project = Project::new(SCHEMA)?;
    let config = project.config(&[
        ("handle", &["alphaNumeric(allowDashes)", "minLength(3)"]),
        ("nickname", &["in(SA, AE, EG)"]),
    ]);
    let dto = project.dto(&config)?;

    let create = struct_block(&dto, "CreateUserDto");
    assert!(
        create.contains(
            "    /// Rules: `alphaNumeric(allowDashes)`, `minLength(3)`.
    #[schema(min_length = 3)]
    #[validate(length(max = 40, message = \"handle must be at most 40 characters\"))]
    #[validate(custom(
        function = \"handle_alpha_numeric\",
        code = \"alphaNumeric\",
        message = \"handle must contain only letters, digits and dashes\"
    ))]
    #[validate(custom(
        function = \"handle_min_length\",
        code = \"minLength\",
        message = \"handle must be at least 3 characters\"
    ))]
    pub handle: Option<String>,"
        ),
        "{create}"
    );
    assert!(
        dto.contains(
            "/// The `minLength(3)` rule of the `handle` column.
fn handle_min_length(value: &str) -> Result<(), validator::ValidationError> {
    check(value.chars().count() >= 3, \"minLength\")
}
"
        ),
        "{dto}"
    );
    assert!(
        dto.contains(
            "fn nickname_in(value: &str) -> Result<(), validator::ValidationError> {
    const VALUES: &[&str] = &[\"SA\", \"AE\", \"EG\"];
    check(VALUES.contains(&value), \"in\")
}
"
        ),
        "{dto}"
    );
    assert!(
        dto.contains("    check(letters(value, true, false, true, false), \"alphaNumeric\")"),
        "{dto}"
    );
    // Each function once, though both bodies use the rules.
    assert_eq!(dto.matches("\nfn check(").count(), 1, "{dto}");
    assert_eq!(dto.matches("\nfn handle_min_length(").count(), 1, "{dto}");
    // No regular expression, so no `regex` hint.
    let db_mod = project.read("src/db/mod.rs")?;
    assert!(!db_mod.contains("regex"), "{db_mod}");
    Ok(())
}

/// A regular expression (`regex`, `mobile`, `postalCode`) is matched with
/// the `regex` crate, compiled once; the data layer's hints name it.
#[test]
fn patterns_need_the_regex_crate() -> Result<()> {
    let project = Project::new(SCHEMA)?;
    let mut config = project.config(&[("handle", &["mobile(ar-SA, strictMode)"])]);
    config
        .api
        .as_mut()
        .expect("the API")
        .rules
        .get_mut("users")
        .expect("the users rules")
        .insert(
            "nickname".into(),
            vec![ApiRule::Regex("^[a-z0-9-]+$".into())],
        );
    let dto = project.dto(&config)?;
    assert!(
        dto.contains(
            "    const PATTERN: &str = \"^[a-z0-9-]+$\";
    static REGEX: std::sync::LazyLock<Option<regex::Regex>> =
        std::sync::LazyLock::new(|| regex::Regex::new(PATTERN).ok());
    check(matches_regex(value, REGEX.as_ref()), \"regex\")"
        ),
        "{dto}"
    );
    assert!(
        dto.contains("    let strict = value.starts_with('+');\n"),
        "{dto}"
    );
    assert!(
        struct_block(&dto, "CreateUserDto").contains("#[schema(pattern = \"^[a-z0-9-]+$\")]"),
        "{dto}"
    );
    let db_mod = project.read("src/db/mod.rs")?;
    assert!(db_mod.contains("//   regex = \"1\"\n"), "{db_mod}");
    Ok(())
}

/// A nullable column's transforms keep `null` meaning "set to `NULL`" in
/// the update body, and an optional create field stays optional.
#[test]
fn transforms_keep_null_and_defaults() -> Result<()> {
    let project = Project::new(SCHEMA)?;
    let config = project.config(&[("nickname", &["trim", "normalizeEmail"])]);
    let dto = project.dto(&config)?;
    assert!(
        struct_block(&dto, "CreateUserDto").contains(
            "    #[serde(default, deserialize_with = \"nickname_transformed\")]\n    pub nickname: Option<String>,"
        ),
        "{dto}"
    );
    assert!(
        struct_block(&dto, "UpdateUserDto").contains(
            "    #[serde(deserialize_with = \"nickname_transformed\")]\n    pub nickname: Option<Option<String>>,"
        ),
        "{dto}"
    );
    assert!(
        dto.contains("    let value = value.trim();\n    normalize_email(value)\n}"),
        "{dto}"
    );
    assert!(
        dto.contains("\nfn normalize_email(value: &str) -> String {"),
        "{dto}"
    );
    Ok(())
}

/// Without the OpenAPI document, no `schema` attribute.
#[test]
fn rules_without_the_document() -> Result<()> {
    let project = Project::new(SCHEMA)?;
    let mut config = project.config(&[("email", &["email", "maxLength(80)"])]);
    config.api.as_mut().expect("the API").openapi = false;
    let dto = project.dto(&config)?;
    assert!(!dto.contains("#[schema("), "{dto}");
    assert!(dto.contains("code = \"maxLength\""), "{dto}");
    Ok(())
}

/// Every rule at once, on long names, is laid out as rustfmt lays it out.
#[test]
fn every_rule_is_rustfmt_clean() -> Result<()> {
    let project = Project::new(
        "CREATE TABLE very_long_named_customer_contact_records (
    id UUID PRIMARY KEY,
    the_primary_contact_email_address_of_this_customer TEXT NOT NULL,
    a TEXT, b TEXT, c TEXT, d TEXT, e TEXT, f TEXT, g TEXT
);",
    )?;
    let mut config = project.config(&[]);
    let long = "the_primary_contact_email_address_of_this_customer";
    let every = [
        "trim",
        "toUpperCase",
        "toLowerCase",
        "normalizeEmail",
        "email",
        "url",
        "uuid(1, 2, 3, 4, 5, 6, 7, 8)",
        "ulid",
        "ipAddress(4)",
        "mobile(ar-SA, ar-AE, ar-EG, en-US, strictMode)",
        "alphaNumeric(allowSpaces, allowDashes, allowUnderscores)",
        "ascii",
        "notIn(alpha, beta, gamma, delta, epsilon, zeta, eta, theta, iota, kappa, lambda, mu, nu, xi)",
        "startsWith(x)",
        "endsWith(y)",
        "minLength(1)",
        "maxLength(4294967295)",
        "fixedLength(10)",
        "hexCode",
        "iban",
        "jwt",
        "postalCode(SA, US, GB, CA)",
        "creditCard(amex, visa, mastercard, dinersclub)",
        "coordinates",
    ];
    let rules: Vec<(&str, Vec<ApiRule>)> = vec![
        (long, every.into_iter().map(ApiRule::from).collect()),
        (
            "a",
            vec![ApiRule::In(vec!["a long value, with a comma".into(); 3])],
        ),
        ("b", vec![ApiRule::Regex("^\"quoted\"\\d+$".into())]),
        (
            "c",
            vec![ApiRule::from("alpha"), ApiRule::from("creditCard")],
        ),
        ("d", vec![ApiRule::from("ipAddress")]),
        ("e", vec![ApiRule::StartsWith("a, b".into())]),
        ("f", vec![ApiRule::from("toUpperCase")]),
        (
            "g",
            vec![ApiRule::from(
                "in(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27)",
            )],
        ),
    ];
    let api = config.api.as_mut().expect("the API");
    api.rules.clear();
    api.rules.insert(
        "very_long_named_customer_contact_records".into(),
        rules
            .into_iter()
            .map(|(column, rules)| (column.to_owned(), rules))
            .collect(),
    );
    rok_sqlc::generate(&config)?;
    let dto = project.path("src/features/very_long_named_customer_contact_records/dto.rs");
    let output = std::process::Command::new("rustfmt")
        .args(["--edition", "2024", "--check"])
        .arg(&dto)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    Ok(())
}
