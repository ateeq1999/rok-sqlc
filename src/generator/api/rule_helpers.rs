//! The helpers the generated checks and transforms of string rules call,
//! written once into each `dto.rs` that needs them, laid out as rustfmt
//! lays them out. Each starts with a newline, the blank line before it.

/// The `check` helper.
pub const CHECK: &str = r#"
/// `Ok` when `valid`, otherwise the error `code`.
fn check(valid: bool, code: &'static str) -> Result<(), validator::ValidationError> {
    if valid {
        Ok(())
    } else {
        Err(validator::ValidationError::new(code))
    }
}
"#;

/// The `matches_regex` helper.
pub const MATCHES_REGEX: &str = r#"
/// Whether `value` matches `regex`, compiled from a pattern rok-sqlc checked.
fn matches_regex(value: &str, regex: Option<&regex::Regex>) -> bool {
    regex.is_some_and(|regex| regex.is_match(value))
}
"#;

/// The `letters` helper.
pub const LETTERS: &str = r#"
/// The `alpha` and `alphaNumeric` rules: one or more ASCII letters (and
/// digits, with `digits`), and whitespace, `-` or `_` where allowed.
fn letters(value: &str, digits: bool, spaces: bool, dashes: bool, underscores: bool) -> bool {
    !value.is_empty()
        && value.chars().all(|c| {
            c.is_ascii_alphabetic()
                || (digits && c.is_ascii_digit())
                || (spaces && c.is_whitespace())
                || (dashes && c == '-')
                || (underscores && c == '_')
        })
}
"#;

/// The `is_uuid` helper.
pub const IS_UUID: &str = r#"
/// validator.js's `isUUID`: hex digits in the 8-4-4-4-12 shape, with the
/// RFC 9562 variant and one of `versions`; with none, any version from 1 to
/// 8, or the nil or max UUID.
fn is_uuid(value: &str, versions: &[u8]) -> bool {
    let bytes = value.as_bytes();
    let shaped = bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        });
    if !shaped {
        return false;
    }
    let lower = value.to_ascii_lowercase();
    if versions.is_empty()
        && (lower == "00000000-0000-0000-0000-000000000000"
            || lower == "ffffffff-ffff-ffff-ffff-ffffffffffff")
    {
        return true;
    }
    let (version, variant) = (bytes[14], bytes[19].to_ascii_lowercase());
    let known = if versions.is_empty() {
        (b'1'..=b'8').contains(&version)
    } else {
        versions.iter().any(|allowed| version == b'0' + allowed)
    };
    known && matches!(variant, b'8' | b'9' | b'a' | b'b')
}
"#;

/// The `is_ulid` helper.
pub const IS_ULID: &str = r#"
/// The `ulid` rule: 26 Crockford base32 characters, the first at most `7`.
fn is_ulid(value: &str) -> bool {
    value.len() == 26
        && value.starts_with(|c: char| ('0'..='7').contains(&c))
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric() && !matches!(c.to_ascii_uppercase(), 'I' | 'L' | 'O' | 'U')
        })
}
"#;

/// The `is_hex_code` helper.
pub const IS_HEX_CODE: &str = r#"
/// The `hexCode` rule: `#` and 3, 4, 6 or 8 hex digits.
fn is_hex_code(value: &str) -> bool {
    value.strip_prefix('#').is_some_and(|hex| {
        matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
    })
}
"#;

/// The `is_jwt` helper.
pub const IS_JWT: &str = r#"
/// validator.js's `isJWT`: three parts joined by `.`, each URL-safe base64
/// without padding (or empty, as validator.js allows).
fn is_jwt(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            part.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
}
"#;

/// The `is_iban` helper.
pub const IS_IBAN: &str = r#"
/// validator.js's `isIBAN`: whitespace and `-` removed and the rest
/// uppercased, a country's format from [`IBAN_FORMATS`], and the mod-97
/// check.
fn is_iban(value: &str) -> bool {
    let iban: String = value
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect::<String>()
        .to_uppercase();
    let Some((_, format)) = IBAN_FORMATS
        .iter()
        .find(|(country, _)| iban.starts_with(country))
    else {
        return false;
    };
    // The format after the country: counts of digits (`n`), letters (`a`)
    // or either (`c`).
    let mut chars = iban.chars().skip(2);
    let mut count = 0;
    for symbol in format.chars() {
        if let Some(digit) = symbol.to_digit(10) {
            count = count * 10 + digit;
            continue;
        }
        for _ in 0..count {
            let fits = chars.next().is_some_and(|c| match symbol {
                'n' => c.is_ascii_digit(),
                'a' => c.is_ascii_uppercase(),
                _ => c.is_ascii_uppercase() || c.is_ascii_digit(),
            });
            if !fits {
                return false;
            }
        }
        count = 0;
    }
    if chars.next().is_some() {
        return false;
    }
    // The first four characters moved to the end, letters as 10 to 35.
    let mut remainder = 0;
    for c in iban.chars().skip(4).chain(iban.chars().take(4)) {
        let Some(value) = c.to_digit(36) else {
            return false;
        };
        let scale = if value < 10 { 10 } else { 100 };
        remainder = (remainder * scale + value) % 97;
    }
    remainder == 1
}
"#;

/// The `is_coordinates` helper.
pub const IS_COORDINATES: &str = r#"
/// validator.js's `isLatLong`: `latitude,longitude` in decimal degrees,
/// optionally in parentheses. As in validator.js, text after a second comma
/// is not read.
fn is_coordinates(value: &str) -> bool {
    let mut parts = value.split(',');
    let (Some(latitude), Some(longitude)) = (parts.next(), parts.next()) else {
        return false;
    };
    if latitude.starts_with('(') != longitude.ends_with(')') {
        return false;
    }
    let latitude = latitude.strip_prefix('(').unwrap_or(latitude);
    let longitude = longitude.strip_suffix(')').unwrap_or(longitude);
    let longitude = longitude
        .strip_prefix(char::is_whitespace)
        .unwrap_or(longitude);
    degrees(latitude, 90) && degrees(longitude, 180)
}
"#;

/// The `degrees` helper.
pub const DEGREES: &str = r#"
/// A signed number of degrees up to `max` (90 or 180), as validator.js's
/// `isLatLong` patterns write one.
fn degrees(text: &str, max: u32) -> bool {
    let text = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (whole, fraction) = text
        .split_once('.')
        .map_or((text, None), |(whole, fraction)| (whole, Some(fraction)));
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    if !digits(whole) || fraction.is_some_and(|fraction| !digits(fraction)) {
        return false;
    }
    let Ok(number) = whole.parse::<u32>() else {
        return false;
    };
    if number == max {
        return fraction.is_none_or(|fraction| fraction.bytes().all(|byte| byte == b'0'));
    }
    if max == 90 {
        whole.len() == 1 || (whole.len() == 2 && (10..90).contains(&number))
    } else {
        whole.len() <= 2 || (whole.len() == 3 && (100..180).contains(&number))
    }
}
"#;

/// The `is_credit_card` helper.
pub const IS_CREDIT_CARD: &str = r#"
/// validator.js's `isCreditCard`: spaces and dashes removed, a number of one
/// of `providers` (of any, when none), passing the Luhn check.
fn is_credit_card(value: &str, providers: &[&str]) -> bool {
    let number: String = value.chars().filter(|c| *c != ' ' && *c != '-').collect();
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let len = number.len();
    let starts = |prefixes: &[&str]| prefixes.iter().any(|prefix| number.starts_with(prefix));
    let between = |at: usize, low: u32, high: u32| {
        number
            .get(at..at + low.to_string().len())
            .and_then(|digits| digits.parse::<u32>().ok())
            .is_some_and(|digits| (low..=high).contains(&digits))
    };
    let of = |provider: &str| match provider {
        "amex" => len == 15 && starts(&["34", "37"]),
        "dinersclub" => len == 14 && (between(0, 300, 305) || starts(&["36", "38"])),
        "discover" => (16..=19).contains(&len) && starts(&["6011", "65"]),
        "jcb" => (len == 15 && starts(&["2131", "1800"])) || (len == 16 && starts(&["35"])),
        // validator.js's pattern: a `51` to `55` prefix, or 16 digits from
        // `2221` to `2720` at the end.
        "mastercard" => {
            (len >= 4 && between(0, 51, 55)) || (len >= 16 && between(len - 16, 2221, 2720))
        }
        "unionpay" => {
            (len == 16 && starts(&["62", "67"])) || ((16..=19).contains(&len) && starts(&["81"]))
        }
        _ => (len == 13 || (16..=19).contains(&len)) && starts(&["4"]),
    };
    let known = if providers.is_empty() {
        CARD_PROVIDERS.iter().any(|provider| of(provider))
    } else {
        providers.iter().any(|provider| of(provider))
    };
    known && luhn(&number)
}
"#;

/// The `luhn` helper.
pub const LUHN: &str = r#"
/// The Luhn check of a number of ASCII digits.
fn luhn(number: &str) -> bool {
    let mut sum = 0;
    for (index, byte) in number.bytes().rev().enumerate() {
        let digit = u32::from(byte - b'0');
        sum += if index % 2 == 1 {
            let doubled = digit * 2;
            if doubled >= 10 { doubled - 9 } else { doubled }
        } else {
            digit
        };
    }
    sum % 10 == 0
}
"#;

/// The `normalize_email` helper.
pub const NORMALIZE_EMAIL: &str = r#"
/// validator.js's `normalizeEmail` with its defaults, which the
/// `normalizeEmail` transform uses: the domain lowercased, and the local part
/// lowercased; for Gmail, the local part without a `+` subaddress or single
/// dots, and `googlemail.com` made `gmail.com`; for iCloud and Outlook.com,
/// without a `+` subaddress; for Yahoo, without a `-` subaddress; for
/// Yandex, the domain made `yandex.ru`. A value without `@`, or whose local
/// part would be left empty, is kept as given.
fn normalize_email(value: &str) -> String {
    let Some((user, domain)) = value.rsplit_once('@') else {
        return value.to_owned();
    };
    let domain = domain.to_lowercase();
    let plus = |user: &str| {
        user.split_once('+')
            .map_or(user, |(user, _)| user)
            .to_owned()
    };
    let (user, domain) = match domain.as_str() {
        "gmail.com" | "googlemail.com" => (remove_single_dots(&plus(user)), "gmail.com".to_owned()),
        name if ICLOUD.contains(&name) || OUTLOOK.contains(&name) => (plus(user), domain),
        name if YAHOO.contains(&name) => {
            let user = user.rsplit_once('-').map_or(user, |(user, _)| user);
            (user.to_owned(), domain)
        }
        name if YANDEX.contains(&name) => (user.to_owned(), "yandex.ru".to_owned()),
        _ => (user.to_owned(), domain),
    };
    if user.is_empty() {
        return value.to_owned();
    }
    format!("{}@{domain}", user.to_lowercase())
}
"#;

/// The `remove_single_dots` helper.
pub const REMOVE_SINGLE_DOTS: &str = r#"
/// `user` without its single dots; a run of two or more stays.
fn remove_single_dots(user: &str) -> String {
    let mut kept = String::new();
    let mut chars = user.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '.' {
            kept.push(c);
            continue;
        }
        let mut run = 1;
        while chars.next_if_eq(&'.').is_some() {
            run += 1;
        }
        if run > 1 {
            kept.push_str(&".".repeat(run));
        }
    }
    kept
}
"#;

/// The `transformed` helper.
pub const TRANSFORMED: &str = r#"
/// A body field a column's transforms apply to: text, optional text, or
/// nullable text, where `null` is `Some(None)`.
trait Transformed<'de>: Sized {
    /// Deserializes the field, transformed by `transform`.
    fn transformed<D>(deserializer: D, transform: fn(&str) -> String) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>;
}

impl<'de> Transformed<'de> for String {
    fn transformed<D>(deserializer: D, transform: fn(&str) -> String) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        serde::Deserialize::deserialize(deserializer).map(|value: String| transform(&value))
    }
}

impl<'de> Transformed<'de> for Option<String> {
    fn transformed<D>(deserializer: D, transform: fn(&str) -> String) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        serde::Deserialize::deserialize(deserializer)
            .map(|value: Option<String>| value.map(|value| transform(&value)))
    }
}

impl<'de> Transformed<'de> for Option<Option<String>> {
    fn transformed<D>(deserializer: D, transform: fn(&str) -> String) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        serde::Deserialize::deserialize(deserializer)
            .map(|value: Option<String>| Some(value.map(|value| transform(&value))))
    }
}
"#;

/// The domains `normalize_email` knows.
pub const EMAIL_DOMAINS: &str = r#"
/// iCloud's domains, as validator.js's `normalizeEmail` lists them.
const ICLOUD: &[&str] = &["icloud.com", "me.com"];

/// Outlook.com's domains, as validator.js's `normalizeEmail` lists them.
const OUTLOOK: &[&str] = &[
    "hotmail.at",
    "hotmail.be",
    "hotmail.ca",
    "hotmail.cl",
    "hotmail.co.il",
    "hotmail.co.nz",
    "hotmail.co.th",
    "hotmail.co.uk",
    "hotmail.com",
    "hotmail.com.ar",
    "hotmail.com.au",
    "hotmail.com.br",
    "hotmail.com.gr",
    "hotmail.com.mx",
    "hotmail.com.pe",
    "hotmail.com.tr",
    "hotmail.com.vn",
    "hotmail.cz",
    "hotmail.de",
    "hotmail.dk",
    "hotmail.es",
    "hotmail.fr",
    "hotmail.hu",
    "hotmail.id",
    "hotmail.ie",
    "hotmail.in",
    "hotmail.it",
    "hotmail.jp",
    "hotmail.kr",
    "hotmail.lv",
    "hotmail.my",
    "hotmail.ph",
    "hotmail.pt",
    "hotmail.sa",
    "hotmail.sg",
    "hotmail.sk",
    "live.be",
    "live.co.uk",
    "live.com",
    "live.com.ar",
    "live.com.mx",
    "live.de",
    "live.es",
    "live.eu",
    "live.fr",
    "live.it",
    "live.nl",
    "msn.com",
    "outlook.at",
    "outlook.be",
    "outlook.cl",
    "outlook.co.il",
    "outlook.co.nz",
    "outlook.co.th",
    "outlook.com",
    "outlook.com.ar",
    "outlook.com.au",
    "outlook.com.br",
    "outlook.com.gr",
    "outlook.com.pe",
    "outlook.com.tr",
    "outlook.com.vn",
    "outlook.cz",
    "outlook.de",
    "outlook.dk",
    "outlook.es",
    "outlook.fr",
    "outlook.hu",
    "outlook.id",
    "outlook.ie",
    "outlook.in",
    "outlook.it",
    "outlook.jp",
    "outlook.kr",
    "outlook.lv",
    "outlook.my",
    "outlook.ph",
    "outlook.pt",
    "outlook.sa",
    "outlook.sg",
    "outlook.sk",
    "passport.com",
];

/// Yahoo's domains, as validator.js's `normalizeEmail` lists them.
const YAHOO: &[&str] = &[
    "rocketmail.com",
    "yahoo.ca",
    "yahoo.co.uk",
    "yahoo.com",
    "yahoo.de",
    "yahoo.fr",
    "yahoo.in",
    "yahoo.it",
    "ymail.com",
];

/// Yandex's domains, as validator.js's `normalizeEmail` lists them.
const YANDEX: &[&str] = &[
    "yandex.ru",
    "yandex.ua",
    "yandex.kz",
    "yandex.com",
    "yandex.by",
    "ya.ru",
];
"#;

/// The providers `is_credit_card` knows.
pub const CARD_PROVIDER_LIST: &str = r#"
/// The card providers validator.js's `isCreditCard` knows.
const CARD_PROVIDERS: &[&str] = &[
    "amex",
    "dinersclub",
    "discover",
    "jcb",
    "mastercard",
    "unionpay",
    "visa",
];
"#;

/// The formats `is_iban` knows.
pub const IBAN_FORMAT_LIST: &str = r#"
/// Each country's IBAN format after its code, as validator.js's `isIBAN`
/// checks it: counts of digits (`n`), uppercase letters (`a`) or either
/// (`c`).
const IBAN_FORMATS: &[(&str, &str)] = &[
    ("AD", "2n8n12c"),
    ("AE", "2n3n16n"),
    ("AL", "2n8n16c"),
    ("AT", "2n16n"),
    ("AZ", "2n4c20n"),
    ("BA", "2n16n"),
    ("BE", "2n12n"),
    ("BG", "2n4a6n8c"),
    ("BH", "2n4a14c"),
    ("BR", "2n23n1a1c"),
    ("BY", "2n4c20n"),
    ("CH", "2n5n12c"),
    ("CR", "2n18n"),
    ("CY", "2n8n16c"),
    ("CZ", "2n20n"),
    ("DE", "2n18n"),
    ("DK", "2n14n"),
    ("DO", "2n4a20n"),
    ("DZ", "24n"),
    ("EE", "2n16n"),
    ("EG", "2n25n"),
    ("ES", "2n20n"),
    ("FI", "2n14n"),
    ("FO", "2n14n"),
    ("FR", "2n10n11c2n"),
    ("GB", "2n4a14n"),
    ("GE", "2n2c16n"),
    ("GI", "2n4a15c"),
    ("GL", "2n14n"),
    ("GR", "2n7n16c"),
    ("GT", "2n4c20c"),
    ("HR", "2n17n"),
    ("HU", "2n24n"),
    ("IE", "2n4a14n"),
    ("IL", "2n19n"),
    ("IQ", "2n4a15n"),
    ("IR", "2n22n"),
    ("IS", "2n22n"),
    ("IT", "2n1a10n12c"),
    ("JO", "2n4a22n"),
    ("KW", "2n4a22c"),
    ("KZ", "2n3n13c"),
    ("LB", "2n4n20c"),
    ("LC", "2n4a24c"),
    ("LI", "2n5n12c"),
    ("LT", "2n16n"),
    ("LU", "2n3n13c"),
    ("LV", "2n4a13c"),
    ("MA", "26n"),
    ("MC", "2n10n11c2n"),
    ("MD", "2n20c"),
    ("ME", "2n18n"),
    ("MK", "2n3n10c2n"),
    ("MR", "2n23n"),
    ("MT", "2n4a5n18c"),
    ("MU", "2n4a19n3a"),
    ("MZ", "2n21n"),
    ("NL", "2n4a10n"),
    ("NO", "2n11n"),
    ("PK", "2n4c16n"),
    ("PL", "2n24n"),
    ("PS", "2n4a21c"),
    ("PT", "2n21n"),
    ("QA", "2n4a21c"),
    ("RO", "2n4a16c"),
    ("RS", "2n18n"),
    ("SA", "2n2n18c"),
    ("SC", "2n4a20n3a"),
    ("SE", "2n20n"),
    ("SI", "2n15n"),
    ("SK", "2n20n"),
    ("SM", "2n1a10n12c"),
    ("SV", "2n4c20n"),
    ("TL", "2n19n"),
    ("TN", "2n20n"),
    ("TR", "2n5n17c"),
    ("UA", "2n6n19c"),
    ("VA", "2n18n"),
    ("VG", "2n4a16n"),
    ("XK", "2n16n"),
];
"#;
